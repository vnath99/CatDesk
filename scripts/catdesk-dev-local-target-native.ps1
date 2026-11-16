# Read-only Windows metadata adapter. No process launch, directory creation,
# target adoption, release authority, or recovery entry point.
function Initialize-DevLocalTargetMetadata {
    $definition = @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace CatDesk.DevLocalTarget {
    public sealed class Observation {
        public string Identity;
        public string Volume;
        public string Owner;
        public string Sha256;
        public long Length;
    }
    public sealed class ReadOnlyScope : IDisposable {
        public const string TemplateSha256 = "__NATIVE_TEMPLATE_SHA256__";
        // Retain every opened component without delete sharing until report exit.
        private readonly List<SafeFileHandle> held = new List<SafeFileHandle>();
        private const uint UnsafeAttributes = 0x400 | 0x1000 | 0x40000 | 0x100000 | 0x400000;
        [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        private static extern SafeFileHandle CreateFile(string path, uint access, uint share,
            IntPtr security, uint disposition, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError=true)]
        private static extern bool GetFileInformationByHandleEx(SafeFileHandle handle,
            int kind, byte[] data, uint size);
        [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        private static extern uint GetFinalPathNameByHandle(SafeFileHandle handle,
            StringBuilder path, uint length, uint flags);
        [DllImport("kernel32.dll", SetLastError=true)]
        private static extern bool GetFileSizeEx(SafeFileHandle handle, out long size);
        [DllImport("kernel32.dll", CharSet=CharSet.Unicode)]
        private static extern uint GetDriveType(string root);
        [DllImport("kernel32.dll")]
        private static extern void GetNativeSystemInfo([Out] byte[] data);
        [DllImport("advapi32.dll")]
        private static extern uint GetSecurityInfo(SafeFileHandle handle, int type, uint info,
            out IntPtr owner, IntPtr group, IntPtr dacl, IntPtr sacl, out IntPtr descriptor);
        [DllImport("kernel32.dll")]
        private static extern IntPtr LocalFree(IntPtr memory);
        [DllImport("shell32.dll")]
        private static extern int SHGetKnownFolderPath(ref Guid id, uint flags,
            IntPtr token, out IntPtr path);

        public static string KnownFolder(bool localAppData) {
            Guid id = new Guid(localAppData ? "f1b32785-6fba-4fcf-9d55-7b8e7f157091"
                                           : "5e6c858f-0e22-4760-9afe-ea3317b67173");
            IntPtr value;
            // DONT_VERIFY avoids requesting folder creation or repair.
            if (SHGetKnownFolderPath(ref id, 0x4000, IntPtr.Zero, out value) < 0)
                throw new InvalidOperationException("KNOWN_FOLDER_UNAVAILABLE");
            try { return Marshal.PtrToStringUni(value); }
            finally { Marshal.FreeCoTaskMem(value); }
        }
        public static string CurrentOwner() {
            using (WindowsIdentity identity = WindowsIdentity.GetCurrent()) {
                if (identity.User == null) throw new InvalidOperationException("OWNER_UNAVAILABLE");
                return identity.User.Value;
            }
        }
        public static bool Amd64Host() {
            byte[] data = new byte[64];
            GetNativeSystemInfo(data);
            return IntPtr.Size == 8 && BitConverter.ToUInt16(data, 0) == 9;
        }
        public static uint OpenFlags(bool directory) {
            // No fallback without NO_RECALL or OPEN_REPARSE_POINT is allowed.
            return 0x00100000u | 0x00200000u | (directory ? 0x02000000u : 0u);
        }
        private static string Full(string path) {
            string full = Path.GetFullPath(path);
            if (full.Length < 3 || full[1] != ':' || full[2] != '\\' ||
                full.StartsWith("\\\\", StringComparison.Ordinal) || full.IndexOf(':', 2) >= 0)
                throw new InvalidOperationException("LOCAL_PATH_REQUIRED");
            return full.Length == 3 ? full : full.TrimEnd('\\');
        }
        private SafeFileHandle Open(string path, bool directory, bool optional, bool readData = false) {
            // Attribute/identity inspection precedes any request for data access.
            uint access = 0x20000 | 0x80 | (readData ? 0x80000000u : 0u);
            SafeFileHandle handle = CreateFile(path, access, directory ? 3u : 1u,
                IntPtr.Zero, 3, OpenFlags(directory), IntPtr.Zero);
            if (handle.IsInvalid) {
                int error = Marshal.GetLastWin32Error(); handle.Dispose();
                if (optional && (error == 2 || error == 3)) return null;
                throw new InvalidOperationException("PATH_OPEN_UNVERIFIED");
            }
            held.Add(handle);
            byte[] attrs = new byte[8];
            if (!GetFileInformationByHandleEx(handle, 9, attrs, 8))
                throw new InvalidOperationException("ATTRIBUTES_UNAVAILABLE");
            uint flags = BitConverter.ToUInt32(attrs, 0);
            if ((flags & UnsafeAttributes) != 0 || ((flags & 0x10) != 0) != directory)
                throw new InvalidOperationException("PATH_REPARSE_CLOUD_OR_KIND_UNSAFE");
            StringBuilder final = new StringBuilder(32768);
            uint count = GetFinalPathNameByHandle(handle, final, 32768, 0);
            if (count == 0 || count >= 32768 ||
                !String.Equals(final.ToString(), "\\\\?\\" + path, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("PATH_ALIAS_UNVERIFIED");
            return handle;
        }
        private SafeFileHandle Capture(string path, bool directory, bool optional) {
            string full = Full(path), root = Path.GetPathRoot(full);
            if (GetDriveType(root) != 3) throw new InvalidOperationException("FIXED_LOCAL_VOLUME_REQUIRED");
            SafeFileHandle leaf = Open(root, true, false);
            if (full.Length == root.Length) return leaf;
            string current = root;
            string[] parts = full.Substring(root.Length).Split('\\');
            if (parts.Length > 64) throw new InvalidOperationException("PATH_DEPTH_REFUSED");
            for (int i = 0; i < parts.Length; i++) {
                current = Path.Combine(current, parts[i]);
                leaf = Open(current, i < parts.Length - 1 || directory, optional);
                if (leaf == null) return null;
            }
            return leaf;
        }
        private Observation Describe(SafeFileHandle handle) {
            byte[] id = new byte[24];
            if (!GetFileInformationByHandleEx(handle, 18, id, 24))
                throw new InvalidOperationException("PHYSICAL_ID_UNAVAILABLE");
            IntPtr owner, descriptor;
            if (GetSecurityInfo(handle, 1, 1, out owner, IntPtr.Zero, IntPtr.Zero,
                    IntPtr.Zero, out descriptor) != 0)
                throw new InvalidOperationException("OWNER_UNAVAILABLE");
            try {
                return new Observation {
                    Volume = BitConverter.ToUInt64(id, 0).ToString("x16"),
                    Identity = BitConverter.ToString(id).Replace("-", "").ToLowerInvariant(),
                    Owner = new SecurityIdentifier(owner).Value
                };
            } finally { if (descriptor != IntPtr.Zero) LocalFree(descriptor); }
        }
        public Observation Directory(string path, bool optional) {
            SafeFileHandle handle = Capture(path, true, optional);
            return handle == null ? null : Describe(handle);
        }
        public Observation File(string path, long maximumBytes, bool optional) {
            SafeFileHandle handle = Capture(path, false, optional);
            if (handle == null) return null;
            Observation result = Describe(handle);
            long length;
            if (!GetFileSizeEx(handle, out length) || length < 0 || length > maximumBytes)
                throw new InvalidOperationException("FILE_SIZE_UNQUALIFIED");
            result.Length = length;
            SafeFileHandle data = Open(Full(path), false, false, true);
            if (Describe(data).Identity != result.Identity)
                throw new InvalidOperationException("FILE_IDENTITY_CHANGED");
            using (SafeFileHandle borrowed = new SafeFileHandle(data.DangerousGetHandle(), false))
            using (FileStream input = new FileStream(borrowed, FileAccess.Read))
            using (SHA256 hash = SHA256.Create()) {
                result.Sha256 = BitConverter.ToString(hash.ComputeHash(input)).Replace("-", "").ToLowerInvariant();
            }
            return result;
        }
        public bool FilePresent(string path) {
            // Configuration existence only: never read/hash unknown config bytes.
            return Capture(path, false, true) != null;
        }
        public void Dispose() {
            for (int i = held.Count - 1; i >= 0; i--) held[i].Dispose();
            held.Clear();
        }
    }
}
'@
    $hash = [Security.Cryptography.SHA256]::Create()
    try { $digest = ([BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($definition)))).Replace('-', '').ToLowerInvariant() }
    finally { $hash.Dispose() }
    if ('CatDesk.DevLocalTarget.ReadOnlyScope' -as [type]) {
        if ([CatDesk.DevLocalTarget.ReadOnlyScope]::TemplateSha256 -cne $digest) { throw 'METADATA_ADAPTER_VERSION_MISMATCH' }
        return
    }
    Add-Type -TypeDefinition $definition.Replace('__NATIVE_TEMPLATE_SHA256__', $digest)
}
