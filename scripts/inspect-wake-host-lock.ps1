param([Parameter(Mandatory=$true)][int]$HostProcessId)
$ErrorActionPreference = 'Stop'
# Read-only diagnosis: enumerate this process's disk handles and return only
# the Wake singleton-lock path. No file contents or process environment read.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class WakeLockPath {
 [DllImport("ntdll.dll")] static extern int NtQuerySystemInformation(int c, IntPtr p, int n, out int needed);
 [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint a, bool inherit, int pid);
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("kernel32.dll")] static extern bool DuplicateHandle(IntPtr source, IntPtr handle, IntPtr target, out IntPtr copy, uint access, bool inherit, uint options);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 [DllImport("kernel32.dll")] static extern uint GetFileType(IntPtr h);
 [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] static extern uint GetFinalPathNameByHandle(IntPtr h, StringBuilder path, uint length, uint flags);
 [DllImport("kernel32.dll")] static extern bool GetFileInformationByHandleEx(IntPtr h, int info, byte[] buffer, uint size);
 [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFile(string path, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
 static string Identity(IntPtr h) { var bytes=new byte[24]; return GetFileInformationByHandleEx(h,18,bytes,24) ? BitConverter.ToString(bytes) : "UNAVAILABLE"; }
 public static string Find(int pid) {
  if (IntPtr.Size != 8) throw new Exception("X64_REQUIRED");
  IntPtr process=OpenProcess(0x40,false,pid);
  if(process==IntPtr.Zero) throw new Exception("PROCESS_HANDLE_UNAVAILABLE");
  IntPtr buffer=IntPtr.Zero;
  try {
   int size=1048576, needed;
   while(true) {
    buffer=Marshal.AllocHGlobal(size);
    int status=NtQuerySystemInformation(64,buffer,size,out needed);
    if(status==0) break;
    Marshal.FreeHGlobal(buffer); buffer=IntPtr.Zero;
    if(status!=unchecked((int)0xC0000004) || size>=67108864) throw new Exception("HANDLE_QUERY_FAILED");
    size=Math.Max(size*2,needed);
   }
   long count=Marshal.ReadInt64(buffer);
   if(count<0 || count>(size-16)/40) throw new Exception("HANDLE_COUNT_INVALID");
   for(long i=0;i<count;i++) {
    IntPtr row=IntPtr.Add(buffer,checked(16+(int)i*40));
    if(Marshal.ReadInt64(row,8)!=pid) continue;
    IntPtr copy;
    if(!DuplicateHandle(process,new IntPtr(Marshal.ReadInt64(row,16)),GetCurrentProcess(),out copy,0,false,2)) continue;
    try {
     if(GetFileType(copy)!=1) continue;
     var path=new StringBuilder(32768);
     uint length=GetFinalPathNameByHandle(copy,path,32768,0);
     if(length>0 && length<32768 && path.ToString().EndsWith(@"\CatDeskWake\host.lock",StringComparison.OrdinalIgnoreCase)) {
      var physical=new StringBuilder(32768);
      GetFinalPathNameByHandle(copy,physical,32768,1);
      IntPtr current=CreateFile(path.ToString(),0,7,IntPtr.Zero,3,0,IntPtr.Zero);
      try { return path.ToString()+"\n"+physical.ToString()+"\nheld="+Identity(copy)+"\npath="+Identity(current); }
      finally { if(current!=new IntPtr(-1)) CloseHandle(current); }
     }
    } finally {CloseHandle(copy);}
   }
   return "WAKE_LOCK_PATH_NOT_FOUND";
  } finally {if(buffer!=IntPtr.Zero) Marshal.FreeHGlobal(buffer); CloseHandle(process);}
 }
}
'@
[WakeLockPath]::Find($HostProcessId)
