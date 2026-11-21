# Define-only output dependency. Loading this file neither compiles nor opens I/O.
function Initialize-DevLocalTargetOutput {
    $definition = @'
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Threading;

namespace CatDesk.DevLocalTarget.OutputV1 {
    internal enum Kind { ConnectPending, Connected, ReadPending, Data, Zero, Eof, Cancelled, Unknown }
    internal struct Step {
        internal Kind Kind; internal int Count;
        internal Step(Kind kind, int count = 0) { Kind = kind; Count = count; }
    }
    internal interface IBackend {
        bool Native { get; }
        long Now { get; }
        void Setup();
        Step Poll(int channel);
        void Copy(int channel, byte[] target, int offset, int count);
        bool Pending { get; }
        bool PendingOn(int channel);
        void Cancel();
        bool CloseSettled();
    }
    internal sealed class Gate {
        private const string Slot = "CatDesk.DevLocalTarget.Output.ProcessGate";
        private readonly Dictionary<string, object> cell;
        private Gate(Dictionary<string, object> value) { cell = value; }
        internal static Gate Mock() { return new Gate(NewCell()); }
        private static Dictionary<string, object> NewCell() {
            return new Dictionary<string, object> { { "schema", 1 }, { "owner", null }, { "poison", false } };
        }
        internal static Gate Process() {
            lock (String.Intern(Slot)) {
                object existing = AppDomain.CurrentDomain.GetData(Slot);
                if (existing == null) {
                    existing = NewCell(); AppDomain.CurrentDomain.SetData(Slot, existing);
                }
                var value = existing as Dictionary<string, object>;
                if (value == null || !value.ContainsKey("schema") || !Object.Equals(value["schema"], 1)
                    || !value.ContainsKey("owner") || !value.ContainsKey("poison"))
                    throw new InvalidOperationException("OUTPUT_PROCESS_GATE_UNVERIFIED");
                return new Gate(value);
            }
        }
        internal Pair Reserve(IBackend backend, int retained) {
            lock (cell) {
                if (!Object.Equals(cell["poison"], false) || cell["owner"] != null)
                    throw new InvalidOperationException("OUTPUT_PROCESS_ADMISSION_HELD");
                var pair = new Pair(backend, this, retained);
                cell["owner"] = pair; // Strong root BEFORE Setup can issue I/O.
                return pair;
            }
        }
        internal void Release(Pair owner) {
            lock (cell) {
                if (!Object.ReferenceEquals(cell["owner"], owner) || !Object.Equals(cell["poison"], false))
                    throw new InvalidOperationException("OUTPUT_PROCESS_GATE_CHANGED");
                cell["owner"] = null;
            }
        }
        internal void Poison(Pair owner) {
            lock (cell) { cell["poison"] = true; if (cell["owner"] == null) cell["owner"] = owner; }
        }
    }
    internal sealed class Capture {
        internal readonly byte[] Buffer;
        internal int Retained;
        internal ulong Total;
        internal bool Overflow, CountSaturated;
        internal string State = "CONNECT_PENDING";
        internal bool Eof;
        private ReadOnlyCollection<byte> frozen;
        internal Capture(int size) { Buffer = new byte[size]; }
        internal void Data(IBackend backend, int channel, int count) {
            if(frozen!=null) throw new InvalidOperationException("OUTPUT_CAPTURE_ALREADY_FROZEN");
            if (count < 0 || count > NativeBackend.Chunk) throw new InvalidOperationException();
            if (UInt64.MaxValue - Total < (ulong)count) { Total = UInt64.MaxValue; CountSaturated = true; }
            else Total += (ulong)count;
            int keep = Math.Min(count, Buffer.Length - Retained);
            if (keep > 0) backend.Copy(channel, Buffer, Retained, keep);
            Retained += keep; Overflow |= keep != count;
        }
        internal ReadOnlyCollection<byte> Frozen() {
            if(frozen==null){var copy=new byte[Retained];System.Buffer.BlockCopy(Buffer,0,copy,0,Retained);frozen=Array.AsReadOnly(copy);}
            return frozen;
        }
    }
    internal sealed class CloseFence {
        internal bool Uncertain;
        internal bool Close(ref IntPtr handle,Func<IntPtr,bool> close) {
            if(Uncertain) return false;
            if(handle==IntPtr.Zero || handle==new IntPtr(-1)) return true;
            Uncertain=true; // Persist BEFORE attempting a possibly uncertain close.
            if(!close(handle)) return false;
            handle=IntPtr.Zero;Uncertain=false;return true;
        }
    }
    public sealed class Observation {
        public string State { get; private set; }
        public string Failure { get; private set; }
        public ReadOnlyCollection<string> Channels { get; private set; }
        public ReadOnlyCollection<bool> Eof { get; private set; }
        public ReadOnlyCollection<ulong> ObservedBytes { get; private set; }
        public ReadOnlyCollection<int> RetainedBytes { get; private set; }
        public ReadOnlyCollection<bool> Overflow { get; private set; }
        public ReadOnlyCollection<bool> CounterSaturated { get; private set; }
        public bool NativeOrigin { get; private set; }
        public bool CancellationRequested { get; private set; }
        public bool IoLifetimeSettled { get; private set; }
        public bool ExecutionQualified { get { return false; } }
        public bool TargetReuseAuthorized { get { return false; } }
        internal Observation(Pair pair) {
            State = pair.State; Failure = pair.Failure; NativeOrigin = pair.Backend.Native;
            IoLifetimeSettled = pair.Settled;
            CancellationRequested = pair.CancelRequested;
            var states=new string[2];var eof=new bool[2];var total=new ulong[2];var retained=new int[2];var overflow=new bool[2];var saturated=new bool[2];
            for (int i=0; i<2; i++) {
                Capture c=pair.Captures[i]; states[i]=c.State; eof[i]=c.Eof;
                total[i]=c.Total; retained[i]=c.Retained; overflow[i]=c.Overflow;
                saturated[i]=c.CountSaturated;
            }
            Channels=Array.AsReadOnly(states);Eof=Array.AsReadOnly(eof);ObservedBytes=Array.AsReadOnly(total);
            RetainedBytes=Array.AsReadOnly(retained);Overflow=Array.AsReadOnly(overflow);
            CounterSaturated=Array.AsReadOnly(saturated);
        }
    }
    public sealed class Pair {
        internal readonly IBackend Backend;
        internal readonly Capture[] Captures;
        private readonly Gate gate;
        private readonly object sync = new object();
        internal string State = "ACTIVE", Failure = null;
        internal bool Settled;
        private bool cancelled, quarantined;
        internal bool CancelRequested { get { return cancelled; } }
        private long cancelledAt;
        internal Pair(IBackend backend, Gate owner, int retained) {
            if (retained < 1 || retained > 16384) throw new InvalidOperationException("OUTPUT_CAPTURE_LIMIT");
            Backend=backend; gate=owner; Captures=new[] {new Capture(retained),new Capture(retained)};
        }
        internal void Setup() {
            try { Backend.Setup(); }
            catch { Failure="OUTPUT_SETUP_UNVERIFIED"; CancelCore(); }
        }
        private Observation View() { return new Observation(this); }
        public Observation Current() {
            if(!Monitor.TryEnter(sync)) throw new InvalidOperationException("OUTPUT_CALL_IN_PROGRESS");
            try{return View();}finally{Monitor.Exit(sync);}
        }
        private void CancelCore() {
            if (cancelled || quarantined || Settled) return;
            cancelled=true; cancelledAt=Backend.Now; State="CANCEL_PENDING";
            try { Backend.Cancel(); } catch { Failure="OUTPUT_CANCEL_UNVERIFIED"; }
        }
        public Observation RequestCancel() {
            if (!Monitor.TryEnter(sync)) throw new InvalidOperationException("OUTPUT_CALL_IN_PROGRESS");
            try { CancelCore(); return View(); } finally { Monitor.Exit(sync); }
        }
        public ReadOnlyCollection<byte> GetCaptured(int channel) {
            if(channel<0 || channel>1) throw new ArgumentOutOfRangeException("channel");
            if(!Monitor.TryEnter(sync)) throw new InvalidOperationException("OUTPUT_CALL_IN_PROGRESS");
            try {
                if(!Settled && !quarantined) throw new InvalidOperationException("OUTPUT_CAPTURE_NOT_FROZEN");
                return Captures[channel].Frozen(); // At most ONE bounded cached copy, never per Poll.
            } finally { Monitor.Exit(sync); }
        }
        public Observation Poll() {
            if (!Monitor.TryEnter(sync)) throw new InvalidOperationException("OUTPUT_CALL_IN_PROGRESS");
            try {
                if (Settled || quarantined) return View();
                for (int i=0; i<2; i++) {
                    Capture c=Captures[i];
                    if (c.Eof || c.State=="SETTLED_CANCELLED") continue;
                    try {
                        if(cancelled && !Backend.PendingOn(i)){c.State="SETTLED_CANCELLED";continue;}
                        Step s=Backend.Poll(i); // At most ONE operation/unit per stream.
                        switch(s.Kind) {
                            case Kind.ConnectPending: c.State="CONNECT_PENDING"; break;
                            case Kind.Connected: c.State="CONNECTED"; break;
                            case Kind.ReadPending: c.State="READ_PENDING"; break;
                            case Kind.Data: c.Data(Backend,i,s.Count); c.State="CONNECTED"; break;
                            case Kind.Zero: c.State="CONNECTED"; break; // Zero bytes is NOT EOF.
                            case Kind.Eof: c.Eof=true; c.State="EOF"; break;
                            case Kind.Cancelled:
                                c.State="SETTLED_CANCELLED";
                                if(!cancelled){Failure="OUTPUT_UNEXPECTED_CANCEL_COMPLETION";CancelCore();}
                                break;
                            default: c.State="ERROR"; Failure="OUTPUT_COMPLETION_UNVERIFIED"; CancelCore(); break;
                        }
                    } catch { c.State="ERROR"; Failure="OUTPUT_OPERATION_UNVERIFIED"; CancelCore(); }
                }
                return View();
            } finally { Monitor.Exit(sync); }
        }
        public Observation TryFinish() {
            if (!Monitor.TryEnter(sync)) throw new InvalidOperationException("OUTPUT_CALL_IN_PROGRESS");
            try {
                if (Settled || quarantined) return View();
                bool eof=Captures[0].Eof && Captures[1].Eof;
                if (!eof && !cancelled && Failure==null) return View();
                bool pending=true;
                try { pending=Backend.Pending; } catch { Failure="OUTPUT_PENDING_QUERY_UNVERIFIED"; CancelCore(); }
                if (pending) {
                    if (cancelled && Backend.Now-cancelledAt >= 1000) Hold();
                    return View();
                }
                try {
                    if (!Backend.CloseSettled()) { Hold(); return View(); }
                    Settled=true;
                    State=Failure!=null ? "ERROR_SETTLED" : cancelled ? (eof?"CANCELLED_AFTER_EOF":"CANCELLED_WITHOUT_EOF") : "DRAINED";
                    gate.Release(this);
                } catch { Hold(); }
                return View();
            } finally { Monitor.Exit(sync); }
        }
        private void Hold() {
            quarantined=true; Settled=false; State="UNKNOWN_QUARANTINED";
            gate.Poison(this); // No Dispose/finalizer/reset/background cleanup.
        }
        internal void FixtureWrite(int channel, byte[] bytes) {
            lock(sync) {
                if (cancelled || quarantined || Settled) throw new InvalidOperationException("OUTPUT_NOT_WRITABLE");
                var native=Backend as NativeBackend;
                if (native==null) throw new InvalidOperationException("OUTPUT_NOT_NATIVE");
                native.FixtureWrite(channel,bytes);
            }
        }
        internal void FixtureCloseWriter(int channel) {
            lock(sync) { ((NativeBackend)Backend).FixtureCloseWriter(channel); }
        }
    }
    internal sealed class NativeBackend : IBackend {
        internal const int Chunk=16384;
        private readonly NativeChannel[] channels = {new NativeChannel(),new NativeChannel()};
        private readonly Stopwatch clock=Stopwatch.StartNew();
        public bool Native { get { return true; } }
        public long Now { get { return clock.ElapsedMilliseconds; } }
        public void Setup() {
            OutputLibrary.NativeSetupEntries++;
            using(WindowsIdentity identity=WindowsIdentity.GetCurrent()) {
                if(identity.User==null) throw new InvalidOperationException();
                string sid=identity.User.Value;
                byte[] random=new byte[16]; using(var rng=RandomNumberGenerator.Create()) rng.GetBytes(random);
                string nonce=BitConverter.ToString(random).Replace("-","").ToLowerInvariant();
                for(int i=0;i<2;i++) channels[i].Setup("\\\\.\\pipe\\LOCAL\\CatDeskDev.Output."+nonce+"."+i,sid);
            }
        }
        public Step Poll(int channel) { return channels[channel].Poll(); }
        public void Copy(int channel,byte[] target,int offset,int count) { Marshal.Copy(channels[channel].Buffer,target,offset,count); }
        public bool Pending { get { return channels[0].Pending || channels[1].Pending; } }
        public bool PendingOn(int channel) { return channels[channel].Pending; }
        public void Cancel() {
            bool failed=false;
            foreach(var c in channels){try{c.Cancel();}catch{failed=true;}}
            if(failed) throw new InvalidOperationException("OUTPUT_CANCEL_UNVERIFIED");
        }
        public bool CloseSettled() {
            if(Pending) return false;
            bool ok=true; foreach(var c in channels) ok=c.CloseSettled() && ok; return ok;
        }
        internal void FixtureWrite(int channel,byte[] bytes) {
            if(channel<0 || channel>1 || bytes==null || bytes.Length>256) throw new InvalidOperationException("OUTPUT_FIXTURE_BOUND");
            channels[channel].FixtureWrite(bytes);
        }
        internal void FixtureCloseWriter(int channel) { if(channel<0 || channel>1) throw new InvalidOperationException(); channels[channel].CloseWriter(); }
    }
    internal sealed class NativeChannel {
        private IntPtr pipe,writer,eventHandle,overlapped;
        internal IntPtr Buffer;
        private int operation; // 0 none, 1 connect, 2 read; retained until completion PROVED.
        private bool connected,ended,stopped;
        private readonly CloseFence closes=new CloseFence();
        private uint token;
        internal bool Pending { get { return operation!=0; } }
        [StructLayout(LayoutKind.Sequential)] private struct SecurityAttributes { internal int Size; internal IntPtr Descriptor; internal int Inherit; }
        [StructLayout(LayoutKind.Sequential)] private struct Overlapped { internal IntPtr Internal,InternalHigh; internal uint Offset,OffsetHigh; internal IntPtr Event; }
        [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] private static extern IntPtr CreateNamedPipe(string name,uint access,uint mode,uint instances,uint output,uint input,uint timeout,ref SecurityAttributes security);
        [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] private static extern IntPtr CreateFile(string name,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
        [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] private static extern IntPtr CreateEvent(IntPtr security,bool manual,bool initial,string name);
        [DllImport("kernel32.dll",SetLastError=true)] private static extern bool ConnectNamedPipe(IntPtr pipe,IntPtr overlapped);
        [DllImport("kernel32.dll",SetLastError=true)] private static extern bool ReadFile(IntPtr pipe,IntPtr buffer,uint size,out uint immediate,IntPtr overlapped);
        [DllImport("kernel32.dll",SetLastError=true)] private static extern bool WriteFile(IntPtr pipe,byte[] buffer,uint size,out uint written,IntPtr overlapped);
        [DllImport("kernel32.dll",SetLastError=true)] private static extern bool GetOverlappedResult(IntPtr pipe,IntPtr overlapped,out uint count,bool wait);
        [DllImport("kernel32.dll",SetLastError=true)] private static extern bool CancelIoEx(IntPtr pipe,IntPtr overlapped);
        [DllImport("kernel32.dll",SetLastError=true)] private static extern bool ResetEvent(IntPtr handle);
        [DllImport("kernel32.dll",SetLastError=true)] private static extern bool CloseHandle(IntPtr handle);
        [DllImport("kernel32.dll",SetLastError=true)] private static extern bool GetHandleInformation(IntPtr handle,out uint flags);
        [DllImport("kernel32.dll",SetLastError=true)] private static extern bool GetNamedPipeClientProcessId(IntPtr pipe,out uint pid);
        [DllImport("kernel32.dll",SetLastError=true)] private static extern bool GetNamedPipeServerProcessId(IntPtr pipe,out uint pid);
        [DllImport("kernel32.dll")] private static extern uint GetCurrentProcessId();
        [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] private static extern bool ConvertStringSecurityDescriptorToSecurityDescriptor(string text,uint revision,out IntPtr descriptor,out uint size);
        [DllImport("advapi32.dll")] private static extern uint GetSecurityInfo(IntPtr handle,int kind,uint info,out IntPtr owner,out IntPtr group,out IntPtr dacl,out IntPtr sacl,out IntPtr descriptor);
        [DllImport("advapi32.dll")] private static extern uint GetSecurityDescriptorLength(IntPtr descriptor);
        [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr value);
        private static bool Valid(IntPtr h) { return h!=IntPtr.Zero && h!=new IntPtr(-1); }
        private static void RequireNoninherit(IntPtr h) { uint flags; if(!GetHandleInformation(h,out flags) || (flags&1)!=0) throw new InvalidOperationException(); }
        internal void Setup(string name,string sid) {
            IntPtr sd=IntPtr.Zero;
            try {
                uint size;
                if(!ConvertStringSecurityDescriptorToSecurityDescriptor("O:"+sid+"D:P(A;;0x1f01ff;;;"+sid+")",1,out sd,out size) || size>4096) throw new InvalidOperationException();
                var security=new SecurityAttributes {Size=Marshal.SizeOf(typeof(SecurityAttributes)),Descriptor=sd,Inherit=0};
                pipe=CreateNamedPipe(name,1u|0x40000000u|0x00080000u,8,1,65536,65536,0,ref security);
                if(!Valid(pipe)) throw new InvalidOperationException();
                RequireNoninherit(pipe); VerifyDacl(pipe,sid);
                eventHandle=CreateEvent(IntPtr.Zero,true,false,null);
                if(!Valid(eventHandle)) throw new InvalidOperationException();
                RequireNoninherit(eventHandle);
                Buffer=Marshal.AllocHGlobal(NativeBackend.Chunk);
                overlapped=Marshal.AllocHGlobal(Marshal.SizeOf(typeof(Overlapped)));
                Prepare(); operation=1;
                bool ok=ConnectNamedPipe(pipe,overlapped); int error=ok?0:Marshal.GetLastWin32Error();
                if(ok || error==535) { operation=0; connected=true; }
                else if(error!=997) throw new InvalidOperationException();
                writer=CreateFile(name,2u|0x80u|0x20000u|0x100000u,0,IntPtr.Zero,3,0,IntPtr.Zero);
                if(!Valid(writer)) throw new InvalidOperationException();
                RequireNoninherit(writer); token=GetCurrentProcessId();
                if(connected) VerifyPeer();
            } finally { if(sd!=IntPtr.Zero) LocalFree(sd); }
        }
        private static void VerifyDacl(IntPtr handle,string sid) {
            IntPtr owner,group,dacl,sacl,descriptor;
            if(GetSecurityInfo(handle,1,5,out owner,out group,out dacl,out sacl,out descriptor)!=0) throw new InvalidOperationException();
            try {
                uint length=GetSecurityDescriptorLength(descriptor);
                if(length==0 || length>4096) throw new InvalidOperationException();
                var bytes=new byte[(int)length]; Marshal.Copy(descriptor,bytes,0,bytes.Length);
                var sd=new RawSecurityDescriptor(bytes,0);
                if(sd.Owner==null || sd.Owner.Value!=sid || (sd.ControlFlags&ControlFlags.DiscretionaryAclProtected)==0 || sd.DiscretionaryAcl==null || sd.DiscretionaryAcl.Count!=1) throw new InvalidOperationException();
                var ace=sd.DiscretionaryAcl[0] as CommonAce;
                if(ace==null || ace.AceQualifier!=AceQualifier.AccessAllowed || ace.IsInherited || ace.SecurityIdentifier.Value!=sid || ace.AccessMask!=0x1f01ff) throw new InvalidOperationException();
            } finally { if(descriptor!=IntPtr.Zero) LocalFree(descriptor); }
        }
        private void VerifyPeer() {
            uint client,server;
            if(!GetNamedPipeClientProcessId(pipe,out client) || !GetNamedPipeServerProcessId(writer,out server) || client!=token || server!=token) throw new InvalidOperationException();
        }
        private void Prepare() {
            if(Pending || !ResetEvent(eventHandle)) throw new InvalidOperationException();
            Marshal.StructureToPtr(new Overlapped {Event=eventHandle},overlapped,false);
        }
        internal Step Poll() {
            if(ended) return new Step(Kind.Cancelled);
            if(Pending) {
                uint count; bool ok=GetOverlappedResult(pipe,overlapped,out count,false);
                int error=ok?0:Marshal.GetLastWin32Error(); int previous=operation;
                if(!ok && error==996) return new Step(previous==1?Kind.ConnectPending:Kind.ReadPending);
                if(!ok && error!=995 && error!=109) return new Step(Kind.Unknown); // Still retain pending storage.
                operation=0;
                if(!ok) { ended=true; return new Step(error==109 && previous==2?Kind.Eof:Kind.Cancelled); }
                if(previous==1) { connected=true; if(stopped){ended=true;return new Step(Kind.Cancelled);} VerifyPeer(); return new Step(Kind.Connected); }
                if(count>NativeBackend.Chunk) return new Step(Kind.Unknown);
                return new Step(count==0?Kind.Zero:Kind.Data,(int)count);
            }
            if(stopped){ended=true;return new Step(Kind.Cancelled);}
            if(!connected) return new Step(Kind.Unknown);
            Prepare(); operation=2;
            uint immediate; bool ready=ReadFile(pipe,Buffer,NativeBackend.Chunk,out immediate,overlapped);
            int code=ready?0:Marshal.GetLastWin32Error();
            // Inline result uses its own count, never a stale signaled event.
            // For ERROR_IO_PENDING discard this count and query completion later.
            if(ready) { operation=0; if(immediate>NativeBackend.Chunk) return new Step(Kind.Unknown); return new Step(immediate==0?Kind.Zero:Kind.Data,(int)immediate); }
            if(code==997) return new Step(Kind.ReadPending);
            if(code==109) { operation=0; ended=true; return new Step(Kind.Eof); }
            return new Step(Kind.Unknown);
        }
        internal void Cancel() {
            stopped=true; // Sticky BEFORE any close/cancel API; observe pending only.
            CloseWriter();
            if(Pending && Valid(pipe)) {
                // Success/NOT_FOUND are requests only: never clear operation.
                if(!CancelIoEx(pipe,overlapped) && Marshal.GetLastWin32Error()!=1168) throw new InvalidOperationException();
            }
        }
        internal void CloseWriter() { if(!closes.Close(ref writer,CloseHandle)) throw new InvalidOperationException("OUTPUT_CLOSE_UNVERIFIED"); }
        internal void FixtureWrite(byte[] bytes) {
            if(operation!=2 || !Valid(writer)) throw new InvalidOperationException("OUTPUT_FIXTURE_REQUIRES_PENDING_READ");
            uint count; if(!WriteFile(writer,bytes,(uint)bytes.Length,out count,IntPtr.Zero) || count!=bytes.Length) throw new InvalidOperationException("OUTPUT_FIXTURE_WRITE_UNVERIFIED");
        }
        internal bool CloseSettled() {
            if(Pending || closes.Uncertain) return false;
            if(!closes.Close(ref writer,CloseHandle) || !closes.Close(ref pipe,CloseHandle) || !closes.Close(ref eventHandle,CloseHandle)) return false;
            if(overlapped!=IntPtr.Zero) { Marshal.FreeHGlobal(overlapped); overlapped=IntPtr.Zero; }
            if(Buffer!=IntPtr.Zero) { Marshal.FreeHGlobal(Buffer); Buffer=IntPtr.Zero; }
            return true;
        }
    }
    internal sealed class FakeBackend : IBackend {
        internal readonly Queue<Step>[] Steps={new Queue<Step>(),new Queue<Step>()};
        internal readonly bool[] Outstanding={false,false};
        internal int Polls,Closes,Cancels,Copies;
        internal long Time;
        internal bool CloseFailure;
        internal bool PendingFailure;
        public bool Native { get { return false; } }
        public long Now { get { return Time; } }
        public bool Pending { get { if(PendingFailure)throw new InvalidOperationException();return Outstanding[0] || Outstanding[1]; } }
        public bool PendingOn(int channel) { return Outstanding[channel]; }
        public void Setup() { }
        internal void Feed(int i,params Step[] steps) { foreach(var s in steps) Steps[i].Enqueue(s); }
        public Step Poll(int i) {
            Polls++; Step s=Steps[i].Count==0?new Step(Kind.ReadPending):Steps[i].Dequeue();
            Outstanding[i]=s.Kind==Kind.ConnectPending || s.Kind==Kind.ReadPending || s.Kind==Kind.Unknown;
            return s;
        }
        public void Copy(int i,byte[] target,int offset,int count) { Copies++; for(int j=0;j<count;j++) target[offset+j]=(byte)(65+i); }
        public void Cancel() { Cancels++; }
        public bool CloseSettled() { Closes++; return !Pending && !CloseFailure; }
    }
    public static class OutputLibrary {
        public const string TemplateSha256="__OUTPUT_SOURCE_SHA256__";
        internal static int NativeSetupEntries,NativeCreatePairEntries;
        public static Pair CreatePair() {
            return CreateNative(16384);
        }
        private static Pair CreateNative(int retained) {
            NativeCreatePairEntries++;
            var pair=Gate.Process().Reserve(new NativeBackend(),retained); pair.Setup(); return pair;
        }
        internal static Pair CreateFixturePair(int retained) { return CreateNative(retained); }
        private static void Check(bool ok,string name,List<string> list) { if(!ok) throw new InvalidOperationException(name); list.Add(name); }
        internal static string[] ManagedChecks() {
            var cases=new List<string>();
            var backend=new FakeBackend(); var gate=Gate.Mock(); var p=gate.Reserve(backend,4);p.Setup();
            Check(!p.Current().NativeOrigin && !p.Current().ExecutionQualified && !p.Current().TargetReuseAuthorized,"mock_not_native_authority",cases);
            bool deniedCapture=false;try{p.GetCaptured(0);}catch(InvalidOperationException){deniedCapture=true;}Check(deniedCapture,"active_capture_access_refused",cases);
            backend.Feed(0,new Step(Kind.ConnectPending),new Step(Kind.Connected),new Step(Kind.ReadPending),new Step(Kind.Zero),new Step(Kind.Data,3),new Step(Kind.Data,3),new Step(Kind.Eof));
            backend.Feed(1,new Step(Kind.Connected),new Step(Kind.ReadPending),new Step(Kind.Data,2),new Step(Kind.ReadPending),new Step(Kind.Eof));
            var s=p.Poll();Check(s.Channels[0]=="CONNECT_PENDING" && s.Channels[1]=="CONNECTED" && backend.Polls==2,"fair_connect_unit_per_channel",cases);
            bool immutable=false;try{((IList<bool>)s.Eof)[0]=true;}catch(NotSupportedException){immutable=true;}Check(immutable,"observation_collections_readonly",cases);
            p.Poll();s=p.Poll();Check(!s.Eof[0] && s.ObservedBytes[1]==2,"sibling_progress_while_read_pending",cases);
            s=p.Poll();Check(!s.Eof[0] && s.Channels[0]=="CONNECTED","zero_completion_not_eof",cases);
            p.Poll();s=p.Poll();Check(s.ObservedBytes[0]==6 && s.RetainedBytes[0]==4 && s.Overflow[0],"cap_retention_continue_drain",cases);
            Check(!p.TryFinish().IoLifetimeSettled && backend.Closes==0,"no_finish_before_both_eof",cases);
            p.Poll();s=p.TryFinish();Check(s.State=="DRAINED" && s.Eof[0] && s.Eof[1] && s.IoLifetimeSettled && backend.Closes==1,"observed_eof_settles_io_only",cases);
            var first=p.GetCaptured(0);var second=p.GetCaptured(1);
            Check(first.Count==4 && first[0]==65 && first[3]==65 && second.Count==2 && second[0]==66,"frozen_prefix_exact_bytes_and_channel_binding",cases);
            Check(Object.ReferenceEquals(first,p.GetCaptured(0)),"captured_copy_cached_not_repeated",cases);
            immutable=false;try{((IList<byte>)first)[0]=0;}catch(NotSupportedException){immutable=true;}Check(immutable,"captured_prefix_readonly",cases);
            var b2=new FakeBackend();var p2=gate.Reserve(b2,4);p2.Setup();
            bool denied=false;try{gate.Reserve(new FakeBackend(),4);}catch(InvalidOperationException){denied=true;}Check(denied,"second_pair_refused_while_active",cases);
            b2.Feed(0,new Step(Kind.ReadPending),new Step(Kind.Cancelled));b2.Feed(1,new Step(Kind.ReadPending),new Step(Kind.Cancelled));
            p2.Poll();p2.RequestCancel();Check(!p2.TryFinish().IoLifetimeSettled && b2.Closes==0,"cancel_request_not_completion",cases);
            p2.Poll();s=p2.TryFinish();Check(s.State=="CANCELLED_WITHOUT_EOF" && s.IoLifetimeSettled && !s.Eof[0] && !s.Eof[1],"cancel_completion_not_eof",cases);
            var connectRace=new FakeBackend();var cr=Gate.Mock().Reserve(connectRace,4);cr.Setup();
            connectRace.Feed(0,new Step(Kind.ConnectPending),new Step(Kind.Connected));connectRace.Feed(1,new Step(Kind.ReadPending),new Step(Kind.Cancelled));
            cr.Poll();cr.RequestCancel();cr.Poll();int calls=connectRace.Polls;s=cr.Poll();
            Check(connectRace.Polls==calls && s.Channels[0]=="SETTLED_CANCELLED" && !s.Eof[0],"normal_connect_after_cancel_never_starts_read",cases);
            var dataRace=new FakeBackend();var dr=Gate.Mock().Reserve(dataRace,4);dr.Setup();
            dataRace.Feed(0,new Step(Kind.ReadPending),new Step(Kind.Data,3));dataRace.Feed(1,new Step(Kind.ReadPending),new Step(Kind.Cancelled));
            dr.Poll();dr.RequestCancel();s=dr.Poll();Check(s.ObservedBytes[0]==3 && s.RetainedBytes[0]==3 && !s.Eof[0],"normal_data_after_cancel_preserved",cases);
            calls=dataRace.Polls;dr.Poll();dr.Poll();Check(dataRace.Polls==calls,"repeated_poll_after_cancel_never_reissues_read",cases);
            var held=new FakeBackend();var holdGate=Gate.Mock();var hp=holdGate.Reserve(held,4);hp.Setup();hp.Poll();hp.RequestCancel();held.Time=999;
            Check(hp.TryFinish().State=="CANCEL_PENDING" && held.Closes==0,"pending_storage_retained_during_grace",cases);
            held.Time=1000;s=hp.TryFinish();Check(s.State=="UNKNOWN_QUARANTINED" && !s.IoLifetimeSettled && held.Closes==0,"unconfirmed_completion_quarantined",cases);
            Check(hp.GetCaptured(0).Count==0 && !hp.Current().IoLifetimeSettled,"quarantine_capture_is_stable_not_cleanup_proof",cases);
            denied=false;try{holdGate.Reserve(new FakeBackend(),4);}catch(InvalidOperationException){denied=true;}Check(denied,"quarantine_denies_new_pairs",cases);
            int polls=held.Polls;hp.Poll();hp.TryFinish();Check(held.Polls==polls && held.Closes==0,"quarantine_has_no_background_or_dispose_probe",cases);
            var bad=new FakeBackend();var bp=Gate.Mock().Reserve(bad,4);bp.Setup();bad.Feed(0,new Step(Kind.Unknown));bad.Feed(1,new Step(Kind.ReadPending));s=bp.Poll();Check(s.Failure!=null && bad.Cancels==1 && !s.IoLifetimeSettled,"unknown_reduces_confidence",cases);
            var closing=new FakeBackend();closing.CloseFailure=true;var cp=Gate.Mock().Reserve(closing,4);cp.Setup();closing.Feed(0,new Step(Kind.Eof));closing.Feed(1,new Step(Kind.Eof));cp.Poll();Check(cp.TryFinish().State=="UNKNOWN_QUARANTINED","close_failure_never_claims_settled",cases);
            int closeCalls=closing.Closes;cp.RequestCancel();cp.Poll();cp.TryFinish();Check(closing.Closes==closeCalls,"held_pair_never_retries_backend_close",cases);
            var fence=new CloseFence();var synthetic=new IntPtr(123);int attempts=0;
            Check(!fence.Close(ref synthetic,h=>{attempts++;return false;}) && fence.Uncertain && synthetic==new IntPtr(123),"fake_close_failure_latches_uncertainty",cases);
            Check(!fence.Close(ref synthetic,h=>{attempts++;return true;}) && attempts==1,"uncertain_raw_handle_never_retried",cases);
            fence=new CloseFence();synthetic=new IntPtr(456);try{fence.Close(ref synthetic,h=>{throw new InvalidOperationException();});}catch(InvalidOperationException){}
            Check(fence.Uncertain && !fence.Close(ref synthetic,h=>true),"throwing_close_keeps_stop_latched",cases);
            var unexpected=new FakeBackend();var up=Gate.Mock().Reserve(unexpected,4);up.Setup();unexpected.Feed(0,new Step(Kind.Cancelled));unexpected.Feed(1,new Step(Kind.ReadPending));Check(up.Poll().Failure=="OUTPUT_UNEXPECTED_CANCEL_COMPLETION","unexpected_cancel_reduces_confidence",cases);
            var query=new FakeBackend();var qp=Gate.Mock().Reserve(query,4);qp.Setup();query.Feed(0,new Step(Kind.Eof));query.Feed(1,new Step(Kind.Eof));qp.Poll();query.PendingFailure=true;
            s=qp.TryFinish();Check(s.State=="CANCEL_PENDING" && query.Cancels==1 && !s.IoLifetimeSettled,"pending_query_failure_starts_cancel_grace",cases);
            query.Time=1000;s=qp.TryFinish();Check(s.State=="UNKNOWN_QUARANTINED" && query.Closes==0,"repeated_pending_query_failure_never_frees",cases);
            var global=new FakeBackend();var gp=Gate.Process().Reserve(global,4);gp.Setup();gp.Poll();gp.RequestCancel();global.Time=1000;gp.TryFinish();
            denied=false;try{Gate.Process().Reserve(new FakeBackend(),4);}catch(InvalidOperationException){denied=true;}Check(denied,"fixed_process_gate_retains_hold_across_gate_instances",cases);
            Check(NativeSetupEntries==0 && NativeCreatePairEntries==0,"fake_route_never_enters_native_setup_or_factory",cases);
            return cases.ToArray();
        }
    }
}
'@
    $hash=[Security.Cryptography.SHA256]::Create()
    try{$digest=([BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($definition)))).Replace('-','').ToLowerInvariant()}finally{$hash.Dispose()}
    if('CatDesk.DevLocalTarget.OutputV1.OutputLibrary' -as [type]){
        if([CatDesk.DevLocalTarget.OutputV1.OutputLibrary]::TemplateSha256 -cne $digest){throw 'OUTPUT_NATIVE_TEMPLATE_MISMATCH'}
        return
    }
    Add-Type -TypeDefinition $definition.Replace('__OUTPUT_SOURCE_SHA256__',$digest)
}
