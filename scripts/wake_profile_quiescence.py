"""Bounded native process metadata only; never open or read a browser/profile.

The existing Rust Job owner runs this helper after START. Command lines remain
local, used only to attribute the selected profile. No PID/path/content leaves
the helper. Quiescence now does not establish any historical submission outcome.
"""
from __future__ import annotations

import ctypes
from ctypes import wintypes as w
import os
from pathlib import Path
import sys

BROWSERS = {"chrome.exe", "chromium.exe"}
DRIVERS = {"chromedriver.exe", "uc_driver.exe", "selenium-manager.exe"}
PYTHONS = {"python.exe", "pythonw.exe"}


def option(args, name):
    values = []
    for index, arg in enumerate(args[1:], 1):
        if arg == name:
            if index + 1 >= len(args) or args[index + 1].startswith("--"):
                raise ValueError("incomplete option")
            values.append(args[index + 1])
        elif arg.startswith(name + "="):
            values.append(arg[len(name) + 1:])
    if len(values) > 1 or any(not value for value in values):
        raise ValueError("ambiguous option")
    return values[0] if values else None


def classify(records, profile, python, samefile=os.path.samefile):
    """Every browser child must reach a live, independently attributed root."""
    by_pid = {row["pid"]: row for row in records}
    if len(by_pid) != len(records):
        raise ValueError("duplicate process")
    other_python = set()
    for row in records:
        if row["name"] in PYTHONS:
            if samefile(row["image"], python):
                return "PROFILE_HELPER_ACTIVE"
            other_python.add(row["pid"])
    roots = set()
    for row in records:
        if row["name"] not in BROWSERS:
            continue
        directory = option(row["args"], "--user-data-dir")
        if directory is not None and samefile(directory, profile):
            return "PROFILE_BROWSER_ACTIVE"
        if option(row["args"], "--type") is None:
            # The qualified login helper always supplies user_data_dir. A
            # default-profile root cannot own that separate explicit profile.
            roots.add(row["pid"])
    for row in records:
        if row["name"] not in BROWSERS | DRIVERS:
            continue
        seen = set()
        current = row
        while current["pid"] not in roots | other_python:
            if current["pid"] in seen:
                raise ValueError("ancestry cycle")
            seen.add(current["pid"])
            parent = by_pid.get(current["parent"])
            if parent is None or parent["created"] > current["created"]:
                return "PROFILE_OWNER_UNATTRIBUTED"
            current = parent
    return "PROFILE_QUIESCENT"


def snapshot(profile, python):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    shell = ctypes.WinDLL("shell32", use_last_error=True)
    ntdll = ctypes.WinDLL("ntdll")
    handle = w.HANDLE
    kernel.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
    kernel.OpenProcess.restype = handle
    kernel.CloseHandle.argtypes = [handle]
    kernel.CloseHandle.restype = w.BOOL
    kernel.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
    kernel.CreateToolhelp32Snapshot.restype = handle

    class Entry(ctypes.Structure):
        _fields_ = [("size", w.DWORD), ("usage", w.DWORD), ("pid", w.DWORD),
                    ("heap", ctypes.c_size_t), ("module", w.DWORD), ("threads", w.DWORD),
                    ("parent", w.DWORD), ("priority", w.LONG), ("flags", w.DWORD),
                    ("name", w.WCHAR * 260)]

    for name in ["Process32FirstW", "Process32NextW"]:
        call = getattr(kernel, name)
        call.argtypes = [handle, ctypes.POINTER(Entry)]
        call.restype = w.BOOL
    kernel.QueryFullProcessImageNameW.argtypes = [handle, w.DWORD, w.LPWSTR, ctypes.POINTER(w.DWORD)]
    kernel.QueryFullProcessImageNameW.restype = w.BOOL
    kernel.GetProcessTimes.argtypes = [handle] + [ctypes.POINTER(w.FILETIME)] * 4
    kernel.GetProcessTimes.restype = w.BOOL
    kernel.WaitForSingleObject.argtypes = [handle, w.DWORD]
    kernel.WaitForSingleObject.restype = w.DWORD
    ntdll.NtQueryInformationProcess.argtypes = [handle, w.ULONG, ctypes.c_void_p, w.ULONG, ctypes.POINTER(w.ULONG)]
    ntdll.NtQueryInformationProcess.restype = w.LONG
    shell.CommandLineToArgvW.argtypes = [w.LPCWSTR, ctypes.POINTER(ctypes.c_int)]
    shell.CommandLineToArgvW.restype = ctypes.POINTER(w.LPWSTR)
    kernel.LocalFree.argtypes = [ctypes.c_void_p]
    kernel.LocalFree.restype = ctypes.c_void_p

    class Unicode(ctypes.Structure):
        _fields_ = [("length", w.USHORT), ("maximum", w.USHORT), ("buffer", ctypes.c_void_p)]

    def command_line(process):
        size = w.ULONG()
        ntdll.NtQueryInformationProcess(process, 60, None, 0, ctypes.byref(size))
        if not ctypes.sizeof(Unicode) <= size.value <= 65536:
            raise ValueError("command line unavailable")
        buffer = ctypes.create_string_buffer(size.value)
        if ntdll.NtQueryInformationProcess(process, 60, buffer, size, ctypes.byref(size)) != 0:
            raise ValueError("command line unavailable")
        value = Unicode.from_buffer(buffer)
        start = ctypes.addressof(buffer)
        if (not value.buffer or value.length == 0 or value.length % 2 or
                value.length > value.maximum or value.buffer < start or
                value.buffer + value.length > start + ctypes.sizeof(buffer)):
            raise ValueError("command line invalid")
        text = ctypes.wstring_at(value.buffer, value.length // 2)
        count = ctypes.c_int()
        args = shell.CommandLineToArgvW(text, ctypes.byref(count))
        if not args:
            raise ValueError("arguments unavailable")
        try:
            if not 1 <= count.value <= 1024:
                raise ValueError("arguments unbounded")
            return list(args[:count.value])
        finally:
            kernel.LocalFree(args)

    invalid = ctypes.c_void_p(-1).value
    table = kernel.CreateToolhelp32Snapshot(2, 0)
    if not table or table == invalid:
        raise ValueError("snapshot unavailable")
    rows, pins = [], []
    try:
        entry = Entry()
        entry.size = ctypes.sizeof(entry)
        ok = kernel.Process32FirstW(table, ctypes.byref(entry))
        if not ok:
            raise ValueError("snapshot empty")
        total = 0
        while ok:
            total += 1
            if total > 16384:
                raise ValueError("snapshot unbounded")
            name = entry.name.lower()
            if name in BROWSERS | DRIVERS | PYTHONS and entry.pid != os.getpid():
                if len(rows) >= 256:
                    raise ValueError("candidate set unbounded")
                process = kernel.OpenProcess(0x1000 | 0x100000, False, entry.pid)
                if not process:
                    raise ValueError("process unavailable")
                pins.append(process)
                image = ctypes.create_unicode_buffer(32768)
                size = w.DWORD(len(image))
                times = [w.FILETIME() for _ in range(4)]
                if (not kernel.QueryFullProcessImageNameW(process, 0, image, ctypes.byref(size)) or
                        not kernel.GetProcessTimes(process, *(ctypes.byref(t) for t in times)) or
                        Path(image.value).name.lower() != name):
                    raise ValueError("process identity unavailable")
                created = times[0].dwLowDateTime | (times[0].dwHighDateTime << 32)
                rows.append(dict(pid=int(entry.pid), parent=int(entry.parent), created=created,
                                 name=name, image=image.value, args=command_line(process)))
            ok = kernel.Process32NextW(table, ctypes.byref(entry))
        if ctypes.get_last_error() != 18:
            raise ValueError("snapshot incomplete")
        state = classify(rows, profile, python)
        if any(kernel.WaitForSingleObject(process, 0) != 258 for process in pins):
            raise ValueError("process changed during observation")
        return state
    finally:
        for process in pins:
            kernel.CloseHandle(process)
        kernel.CloseHandle(table)


def main():
    if len(sys.argv) != 3 or sys.stdin.readline() != "START\n":
        return 2
    try:
        profile, python = map(Path, sys.argv[1:])
        if not profile.is_absolute() or not profile.is_dir() or not python.is_absolute() or not python.is_file():
            raise ValueError("selected roots unavailable")
        state = snapshot(profile, python)
    except Exception:
        state = "PROFILE_INVENTORY_UNVERIFIED"
    print("PROBE:FINAL:" + state, flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
