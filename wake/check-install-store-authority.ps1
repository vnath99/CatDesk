param(
    [Parameter(Mandatory=$true)][string]$WakeRoot,
    [Parameter(Mandatory=$true)][int]$HostProcessId,
    [switch]$RequireLiveRootProof
)
$ErrorActionPreference = 'Stop'

# This is a read-only, fail-closed identity probe. The caller supplies the PID
# only after independently verifying the single installed WakeHost process.
# In particular, the displayed final path is never accepted as object identity:
# packaged Windows processes can resolve that path to different file objects.
if (-not ('CatDeskWakeInstallStoreAuthority' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;

public static class CatDeskWakeInstallStoreAuthority {
    const int SystemExtendedHandleInformation = 64;
    const int StatusInfoLengthMismatch = unchecked((int)0xC0000004);
    const uint ProcessDuplicateHandle = 0x40;
    const uint DuplicateSameAccess = 2;
    const uint FileTypeDisk = 1;
    const uint OpenExisting = 3;
    const uint ShareAll = 7;
    const uint ShareWithoutDelete = 3;
    const uint BackupSemantics = 0x02000000;
    const uint OpenReparsePoint = 0x00200000;
    const uint AttributeDirectory = 0x10;
    const uint AttributeReparsePoint = 0x400;
    const int ErrorAlreadyExists = 183;
    const int MaxHandleTableBytes = 128 * 1024 * 1024;

    [StructLayout(LayoutKind.Sequential)]
    internal struct FileInformation {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME Creation;
        public System.Runtime.InteropServices.ComTypes.FILETIME Access;
        public System.Runtime.InteropServices.ComTypes.FILETIME Write;
        public uint Volume;
        public uint SizeHigh;
        public uint SizeLow;
        public uint Links;
        public uint IndexHigh;
        public uint IndexLow;
    }

    [DllImport("ntdll.dll")]
    static extern int NtQuerySystemInformation(int kind, IntPtr buffer, int length, out int needed);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")]
    static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool DuplicateHandle(IntPtr source, IntPtr handle, IntPtr target,
        out IntPtr copy, uint access, bool inherit, uint options);
    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")]
    static extern uint GetFileType(IntPtr handle);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)]
    static extern uint GetFinalPathNameByHandle(IntPtr handle, StringBuilder path,
        uint length, uint flags);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr CreateFile(string path, uint access, uint share,
        IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern bool CreateDirectory(string path, IntPtr security);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool GetFileInformationByHandle(IntPtr handle, out FileInformation information);

    static void Require(bool valid, string error) {
        if (!valid) throw new InvalidOperationException(error);
    }

    static IntPtr OpenNoFollow(string path, bool directory) {
        IntPtr handle = CreateFile(path, 0, ShareAll, IntPtr.Zero, OpenExisting,
            OpenReparsePoint | (directory ? BackupSemantics : 0), IntPtr.Zero);
        Require(handle != IntPtr.Zero && handle != new IntPtr(-1),
            "WAKE_STORE_AUTHORITY_OBJECT_UNAVAILABLE");
        return handle;
    }

    static IntPtr OpenPinned(string path, bool directory) {
        IntPtr handle = CreateFile(path, 0, ShareWithoutDelete, IntPtr.Zero, OpenExisting,
            OpenReparsePoint | (directory ? BackupSemantics : 0), IntPtr.Zero);
        Require(handle != IntPtr.Zero && handle != new IntPtr(-1),
            "WAKE_STORE_AUTHORITY_PIN_UNAVAILABLE");
        return handle;
    }

    public sealed class PinnedStore : IDisposable {
        IntPtr root;
        IntPtr lockFile;
        IntPtr versions;
        readonly string path;
        readonly FileInformation rootInfo;
        readonly FileInformation lockInfo;
        FileInformation versionsInfo;

        internal PinnedStore(string path, IntPtr root, IntPtr lockFile,
            FileInformation rootInfo, FileInformation lockInfo) {
            this.path = path;
            this.root = root;
            this.lockFile = lockFile;
            this.rootInfo = rootInfo;
            this.lockInfo = lockInfo;
        }

        public void Verify() {
            Require(root != IntPtr.Zero && lockFile != IntPtr.Zero,
                "WAKE_STORE_AUTHORITY_PIN_CLOSED");
            IntPtr currentRoot = OpenNoFollow(path, true);
            IntPtr currentLock = IntPtr.Zero;
            try {
                currentLock = OpenNoFollow(Path.Combine(path, "host.lock"), false);
                Require(SameObject(Info(currentRoot), rootInfo) &&
                    SameObject(Info(currentLock), lockInfo),
                    "WAKE_STORE_AUTHORITY_PIN_CHANGED");
                if (versions != IntPtr.Zero) {
                    IntPtr currentVersions = OpenNoFollow(Path.Combine(path, "versions"), true);
                    try {
                        FileInformation info = Info(currentVersions);
                        Require((info.Attributes & (AttributeDirectory | AttributeReparsePoint)) ==
                            AttributeDirectory && SameObject(info, versionsInfo),
                            "WAKE_STORE_AUTHORITY_VERSIONS_CHANGED");
                    } finally { CloseHandle(currentVersions); }
                }
            } finally {
                if (currentLock != IntPtr.Zero) CloseHandle(currentLock);
                CloseHandle(currentRoot);
            }
        }

        public void PinVersions() {
            Require(versions == IntPtr.Zero, "WAKE_STORE_AUTHORITY_VERSIONS_ALREADY_PINNED");
            string pathToVersions = Path.Combine(path, "versions");
            if (!CreateDirectory(pathToVersions, IntPtr.Zero)) {
                Require(Marshal.GetLastWin32Error() == ErrorAlreadyExists,
                    "WAKE_STORE_AUTHORITY_VERSIONS_UNAVAILABLE");
            }
            IntPtr held = OpenPinned(pathToVersions, true);
            try {
                FileInformation info = Info(held);
                Require((info.Attributes & (AttributeDirectory | AttributeReparsePoint)) ==
                    AttributeDirectory, "WAKE_STORE_AUTHORITY_VERSIONS_INVALID");
                versionsInfo = info;
                versions = held;
            } catch { CloseHandle(held); throw; }
        }

        public void Dispose() {
            if (versions != IntPtr.Zero) { CloseHandle(versions); versions = IntPtr.Zero; }
            if (lockFile != IntPtr.Zero) { CloseHandle(lockFile); lockFile = IntPtr.Zero; }
            if (root != IntPtr.Zero) { CloseHandle(root); root = IntPtr.Zero; }
        }
    }

    public static PinnedStore Pin(string root, string expectedFingerprint) {
        string path = SafeRoot(root);
        IntPtr rootHandle = OpenPinned(path, true);
        IntPtr lockHandle = IntPtr.Zero;
        try {
            lockHandle = OpenPinned(Path.Combine(path, "host.lock"), false);
            FileInformation rootInfo = Info(rootHandle);
            FileInformation lockInfo = Info(lockHandle);
            Require((rootInfo.Attributes & (AttributeDirectory | AttributeReparsePoint)) ==
                AttributeDirectory &&
                (lockInfo.Attributes & (AttributeDirectory | AttributeReparsePoint)) == 0 &&
                lockInfo.Links == 1,
                "WAKE_STORE_AUTHORITY_PIN_INVALID");
            Require(String.Equals(Fingerprint(rootInfo, lockInfo), expectedFingerprint,
                StringComparison.OrdinalIgnoreCase),
                "WAKE_STORE_AUTHORITY_PIN_MISMATCH");
            return new PinnedStore(path, rootHandle, lockHandle, rootInfo, lockInfo);
        } catch {
            if (lockHandle != IntPtr.Zero) CloseHandle(lockHandle);
            CloseHandle(rootHandle);
            throw;
        }
    }

    static FileInformation Info(IntPtr handle) {
        FileInformation information;
        Require(GetFileInformationByHandle(handle, out information),
            "WAKE_STORE_AUTHORITY_OBJECT_INFO_UNAVAILABLE");
        return information;
    }

    static bool SameObject(FileInformation left, FileInformation right) {
        return left.Volume == right.Volume && left.IndexHigh == right.IndexHigh
            && left.IndexLow == right.IndexLow;
    }

    static string Identity(FileInformation information) {
        return information.Volume.ToString("x8") + ":" +
            information.IndexHigh.ToString("x8") + ":" +
            information.IndexLow.ToString("x8");
    }

    static string Fingerprint(FileInformation root, FileInformation lockFile) {
        byte[] bytes = Encoding.UTF8.GetBytes("catdesk-wake-install-store-v1\0" +
            Identity(root) + "/" + Identity(lockFile));
        using (SHA256 sha = SHA256.Create()) {
            byte[] digest = sha.ComputeHash(bytes);
            StringBuilder output = new StringBuilder(64);
            foreach (byte value in digest) output.Append(value.ToString("x2"));
            return output.ToString();
        }
    }

    static string SafeRoot(string root) {
        Require(!String.IsNullOrWhiteSpace(root), "WAKE_STORE_AUTHORITY_ROOT_INVALID");
        string full = Path.GetFullPath(root).TrimEnd('\\');
        Require(Path.IsPathRooted(full) && !full.StartsWith("\\\\", StringComparison.Ordinal)
            && String.Equals(full, root.TrimEnd('\\'), StringComparison.OrdinalIgnoreCase),
            "WAKE_STORE_AUTHORITY_ROOT_INVALID");
        string drive = Path.GetPathRoot(full);
        Require(drive != null && drive.Length == 3 && drive[1] == ':',
            "WAKE_STORE_AUTHORITY_ROOT_INVALID");
        string current = drive;
        foreach (string part in full.Substring(drive.Length).Split('\\')) {
            Require(part.Length > 0 && part != "." && part != "..",
                "WAKE_STORE_AUTHORITY_ROOT_INVALID");
            current = Path.Combine(current, part);
            IntPtr component = OpenNoFollow(current, true);
            try {
                FileInformation info = Info(component);
                Require((info.Attributes & AttributeDirectory) != 0 &&
                    (info.Attributes & AttributeReparsePoint) == 0,
                    "WAKE_STORE_AUTHORITY_ROOT_INVALID");
            } finally { CloseHandle(component); }
        }
        return full;
    }

    public static string Probe(string root, int pid, bool requireLiveRoot) {
        Require(IntPtr.Size == 8 && pid > 0, "WAKE_STORE_AUTHORITY_PLATFORM_INVALID");
        string canonicalRoot = SafeRoot(root);
        string expectedLock = "\\\\?\\" + Path.Combine(canonicalRoot, "host.lock");
        IntPtr rootHandle = OpenNoFollow(canonicalRoot, true);
        IntPtr lockHandle = IntPtr.Zero;
        IntPtr process = IntPtr.Zero;
        IntPtr table = IntPtr.Zero;
        try {
            FileInformation rootInfo = Info(rootHandle);
            Require((rootInfo.Attributes & AttributeDirectory) != 0 &&
                (rootInfo.Attributes & AttributeReparsePoint) == 0,
                "WAKE_STORE_AUTHORITY_ROOT_INVALID");
            lockHandle = OpenNoFollow(Path.Combine(canonicalRoot, "host.lock"), false);
            FileInformation lockInfo = Info(lockHandle);
            Require((lockInfo.Attributes & (AttributeDirectory | AttributeReparsePoint)) == 0
                && lockInfo.Links == 1, "WAKE_STORE_AUTHORITY_LOCK_INVALID");
            process = OpenProcess(ProcessDuplicateHandle, false, pid);
            Require(process != IntPtr.Zero, "WAKE_STORE_AUTHORITY_PROCESS_UNAVAILABLE");

            int size = 1024 * 1024;
            int needed;
            while (true) {
                Require(size <= MaxHandleTableBytes,
                    "WAKE_STORE_AUTHORITY_HANDLE_TABLE_TOO_LARGE");
                table = Marshal.AllocHGlobal(size);
                int result = NtQuerySystemInformation(SystemExtendedHandleInformation,
                    table, size, out needed);
                if (result == 0) break;
                Marshal.FreeHGlobal(table);
                table = IntPtr.Zero;
                Require(result == StatusInfoLengthMismatch,
                    "WAKE_STORE_AUTHORITY_HANDLE_QUERY_FAILED");
                size = Math.Max(size * 2, needed);
            }
            long count = Marshal.ReadInt64(table);
            Require(count >= 0 && count <= (size - 16) / 40,
                "WAKE_STORE_AUTHORITY_HANDLE_COUNT_INVALID");
            int matches = 0;
            int rootMatches = 0;
            FileInformation heldInfo = new FileInformation();
            for (long index = 0; index < count; index++) {
                IntPtr row = IntPtr.Add(table, checked(16 + (int)index * 40));
                if (Marshal.ReadInt64(row, 8) != pid) continue;
                IntPtr copy;
                if (!DuplicateHandle(process, new IntPtr(Marshal.ReadInt64(row, 16)),
                    GetCurrentProcess(), out copy, 0, false, DuplicateSameAccess)) continue;
                try {
                    if (GetFileType(copy) != FileTypeDisk) continue;
                    FileInformation candidateInfo;
                    if (GetFileInformationByHandle(copy, out candidateInfo) &&
                        (candidateInfo.Attributes & AttributeDirectory) != 0 &&
                        (candidateInfo.Attributes & AttributeReparsePoint) == 0 &&
                        SameObject(candidateInfo, rootInfo)) {
                        rootMatches++;
                        Require(rootMatches == 1,
                            "WAKE_STORE_AUTHORITY_LIVE_ROOT_AMBIGUOUS");
                    }
                    StringBuilder path = new StringBuilder(32768);
                    uint length = GetFinalPathNameByHandle(copy, path, 32768, 0);
                    if (length == 0 || length >= 32768 ||
                        !String.Equals(path.ToString(), expectedLock,
                            StringComparison.OrdinalIgnoreCase)) continue;
                    matches++;
                    Require(matches == 1, "WAKE_STORE_AUTHORITY_LOCK_AMBIGUOUS");
                    heldInfo = Info(copy);
                    Require((heldInfo.Attributes & (AttributeDirectory | AttributeReparsePoint)) == 0
                        && heldInfo.Links == 1, "WAKE_STORE_AUTHORITY_HELD_LOCK_INVALID");
                } finally { CloseHandle(copy); }
            }
            Require(matches == 1, "WAKE_STORE_AUTHORITY_HELD_LOCK_NOT_FOUND");
            Require(SameObject(heldInfo, lockInfo),
                "WAKE_STORE_AUTHORITY_LOCK_OBJECT_MISMATCH");
            if (requireLiveRoot) {
                Require(rootMatches == 1, "WAKE_STORE_AUTHORITY_LIVE_ROOT_UNPROVEN");
            }
            return Fingerprint(rootInfo, lockInfo);
        } finally {
            if (table != IntPtr.Zero) Marshal.FreeHGlobal(table);
            if (process != IntPtr.Zero) CloseHandle(process);
            if (lockHandle != IntPtr.Zero) CloseHandle(lockHandle);
            CloseHandle(rootHandle);
        }
    }
}
'@
}

$fingerprint = [CatDeskWakeInstallStoreAuthority]::Probe(
    $WakeRoot, $HostProcessId, [bool]$RequireLiveRootProof
)
$fingerprint
