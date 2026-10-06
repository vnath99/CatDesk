[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Listener', 'Process', 'Daemons')]
    [string]$Mode,
    [int]$Port = 0,
    [int]$ProcessId = 0
)

$ErrorActionPreference = 'Stop'

function Write-InventoryRows {
    param([object[]]$Rows)
    [pscustomobject]@{ Rows = @($Rows) } | ConvertTo-Json -Compress -Depth 4
}

function Initialize-LocalProcessCommandLineReader {
    if ('CatDesk.LocalProcessCommandLineReader' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Net;
using System.Runtime.InteropServices;

namespace CatDesk {
    public static class LocalProcessCommandLineReader {
        private const uint ProcessQueryLimitedInformation = 0x1000;
        private const int ProcessCommandLineInformation = 60;
        private const int AfInet = 2;
        private const int AfInet6 = 23;
        private const int TcpTableOwnerPidListener = 3;
        private const uint ErrorInsufficientBuffer = 122;

        [StructLayout(LayoutKind.Sequential)]
        private struct UnicodeString {
            public ushort Length;
            public ushort MaximumLength;
            public IntPtr Buffer;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct Tcp4Row {
            public uint State;
            public uint LocalAddress;
            public uint LocalPort;
            public uint RemoteAddress;
            public uint RemotePort;
            public uint OwningPid;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct Tcp6Row {
            [MarshalAs(UnmanagedType.ByValArray, SizeConst = 16)] public byte[] LocalAddress;
            public uint LocalScopeId;
            public uint LocalPort;
            [MarshalAs(UnmanagedType.ByValArray, SizeConst = 16)] public byte[] RemoteAddress;
            public uint RemoteScopeId;
            public uint RemotePort;
            public uint State;
            public uint OwningPid;
        }

        public sealed class LoopbackListener {
            public int Port;
            public int ProcessId;
            public string LocalAddress;
        }

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr OpenProcess(uint desiredAccess, bool inheritHandle, int processId);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CloseHandle(IntPtr handle);

        [DllImport("ntdll.dll")]
        private static extern int NtQueryInformationProcess(
            IntPtr processHandle,
            int processInformationClass,
            IntPtr processInformation,
            int processInformationLength,
            out int returnLength);

        [DllImport("iphlpapi.dll")]
        private static extern uint GetExtendedTcpTable(
            IntPtr table,
            ref int size,
            bool order,
            int addressFamily,
            int tableClass,
            uint reserved);

        private static int Port(uint value) {
            return (int)(((value & 0xff) << 8) | ((value >> 8) & 0xff));
        }

        private static IntPtr ReadTable(int addressFamily, out int count) {
            int size = 0;
            uint first = GetExtendedTcpTable(IntPtr.Zero, ref size, false, addressFamily, TcpTableOwnerPidListener, 0);
            if (first != ErrorInsufficientBuffer || size < 4 || size > 8 * 1024 * 1024) throw new InvalidOperationException("TCP listener inventory is unavailable");
            IntPtr table = Marshal.AllocHGlobal(size);
            uint result = GetExtendedTcpTable(table, ref size, false, addressFamily, TcpTableOwnerPidListener, 0);
            if (result != 0) { Marshal.FreeHGlobal(table); throw new InvalidOperationException("TCP listener inventory is unavailable"); }
            count = Marshal.ReadInt32(table);
            if (count < 0 || count > 65536) { Marshal.FreeHGlobal(table); throw new InvalidOperationException("TCP listener inventory is ambiguous"); }
            return table;
        }

        public static LoopbackListener[] ReadLoopbackListeners() {
            var results = new List<LoopbackListener>();
            int count4;
            IntPtr v4 = ReadTable(AfInet, out count4);
            try {
                int size4 = Marshal.SizeOf(typeof(Tcp4Row));
                for (int i = 0; i < count4; i++) {
                    var row = (Tcp4Row)Marshal.PtrToStructure(IntPtr.Add(v4, 4 + i * size4), typeof(Tcp4Row));
                    var address = new IPAddress(BitConverter.GetBytes(row.LocalAddress));
                    if (row.State == 2 && IPAddress.IsLoopback(address)) results.Add(new LoopbackListener { Port = Port(row.LocalPort), ProcessId = unchecked((int)row.OwningPid), LocalAddress = address.ToString() });
                }
            } finally { Marshal.FreeHGlobal(v4); }
            int count6;
            IntPtr v6 = ReadTable(AfInet6, out count6);
            try {
                int size6 = Marshal.SizeOf(typeof(Tcp6Row));
                for (int i = 0; i < count6; i++) {
                    var row = (Tcp6Row)Marshal.PtrToStructure(IntPtr.Add(v6, 4 + i * size6), typeof(Tcp6Row));
                    var address = new IPAddress(row.LocalAddress);
                    if (row.State == 2 && IPAddress.IsLoopback(address)) results.Add(new LoopbackListener { Port = Port(row.LocalPort), ProcessId = unchecked((int)row.OwningPid), LocalAddress = address.ToString() });
                }
            } finally { Marshal.FreeHGlobal(v6); }
            return results.ToArray();
        }

        public static string Read(int processId) {
            if (processId < 1) return null;
            IntPtr process = OpenProcess(ProcessQueryLimitedInformation, false, processId);
            if (process == IntPtr.Zero) return null;
            try {
                int required;
                NtQueryInformationProcess(
                    process,
                    ProcessCommandLineInformation,
                    IntPtr.Zero,
                    0,
                    out required);
                if (required < Marshal.SizeOf(typeof(UnicodeString)) || required > 65536) return null;
                IntPtr buffer = Marshal.AllocHGlobal(required);
                try {
                    int returned;
                    if (NtQueryInformationProcess(
                        process,
                        ProcessCommandLineInformation,
                        buffer,
                        required,
                        out returned) != 0) return null;
                    UnicodeString value = (UnicodeString)Marshal.PtrToStructure(buffer, typeof(UnicodeString));
                    if (value.Buffer == IntPtr.Zero || value.Length == 0 || (value.Length & 1) != 0) return null;
                    return Marshal.PtrToStringUni(value.Buffer, value.Length / 2);
                } finally {
                    Marshal.FreeHGlobal(buffer);
                }
            } finally {
                CloseHandle(process);
            }
        }
    }
}
'@
}

function Get-LocalProcessInventoryRow {
    param([int]$ProcessId)
    if ($ProcessId -lt 1) { return $null }
    Initialize-LocalProcessCommandLineReader
    try { $process = Get-Process -Id $ProcessId -ErrorAction Stop } catch { return $null }
    $commandLine = [CatDesk.LocalProcessCommandLineReader]::Read($ProcessId)
    if ([string]::IsNullOrWhiteSpace($commandLine)) { throw 'CatDesk process command line is unverifiable' }
    $creation = $null
    try { $creation = $process.StartTime.ToUniversalTime().ToString('o') } catch {}
    $path = $null
    try { $path = [string]$process.Path } catch {}
    if ([string]::IsNullOrWhiteSpace($creation) -or [string]::IsNullOrWhiteSpace($path)) {
        throw 'CatDesk process identity is ambiguous'
    }
    return [pscustomobject]@{
        Name = ([string]$process.ProcessName + '.exe')
        ProcessId = $ProcessId
        ExecutablePath = $path
        CommandLine = $commandLine
        CreationDate = $creation
    }
}

switch ($Mode) {
    'Listener' {
        if ($Port -lt 1 -or $Port -gt 65535) { throw 'inventory listener port is invalid' }
        $rows = @(
            Initialize-LocalProcessCommandLineReader
            [CatDesk.LocalProcessCommandLineReader]::ReadLoopbackListeners() |
                Where-Object { $_.Port -eq $Port } |
                Select-Object -First 3 |
                ForEach-Object {
                    [pscustomobject]@{
                        LocalAddress = [string]$_.LocalAddress
                        OwningProcess = [int]$_.ProcessId
                    }
                }
        )
        Write-InventoryRows -Rows $rows
        break
    }
    'Process' {
        if ($ProcessId -lt 1) { throw 'inventory process id is invalid' }
        $row = Get-LocalProcessInventoryRow -ProcessId $ProcessId
        $rows = if ($null -eq $row) { @() } else { @($row) }
        Write-InventoryRows -Rows $rows
        break
    }
    'Daemons' {
        # Win32_Process command-line reads are denied on this supported user-level
        # recovery path. Query limited-information handles locally instead. The
        # helper only narrows observation; the parent still owns exact daemon-token,
        # canonical path/SHA-256, creation, pinned-handle, listener, and ambiguity
        # authority. Bound the relevant token-matching set, not arbitrary same-image
        # rows. A separate raw snapshot ceiling prevents unbounded local inspection.
        $processes = @(Get-Process -Name 'catdesk' -ErrorAction SilentlyContinue | Select-Object -First 257)
        if ($processes.Count -gt 256) { throw 'too many CatDesk process rows are ambiguous' }
        $relevant = @()
        foreach ($process in $processes) {
            $row = Get-LocalProcessInventoryRow -ProcessId ([int]$process.Id)
            if ($null -eq $row) { continue }
            if ([string]$row.CommandLine -notmatch '(?i)(?:^|\s)--catdesk-daemon(?:\s|$)') { continue }
            $relevant += $row
            if ($relevant.Count -gt 64) { break }
        }
        $rows = @($relevant | Select-Object -First 65)
        Write-InventoryRows -Rows $rows
        break
    }
}
