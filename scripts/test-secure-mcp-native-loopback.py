"""Opt-in local test of the pinned Windows tunnel client and CatDesk bearer injection."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

os.chdir(Path(__file__).resolve().parents[1])
client = Path(os.environ['USERPROFILE']) / '.catdesk/tools/tunnel-client/current/tunnel-client.exe'
client_sha = 'fcc85a69ec0ad82518e4f8964f60c45e31787957782a0fc9c1b0c44e82d61b9b'
if hashlib.sha256(client.read_bytes()).hexdigest() != client_sha:
    raise SystemExit('PINNED_TUNNEL_CLIENT_IDENTITY_CHANGED')
ps = r"""
$ErrorActionPreference='Stop'
$parseTokens=$null; $parseErrors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location).Path 'scripts/setup-secure-mcp.ps1'),[ref]$parseTokens,[ref]$parseErrors)
if ($parseErrors.Count) { throw 'PARSE' }
foreach($functionName in @('ConvertTo-NativeCommandLineArgument','Initialize-BoundedNativeProcessType','Invoke-BoundedTunnelClient','Get-LocalMcpRuntimeAuthorization')){
    $node=$ast.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $functionName},$true)
    . ([scriptblock]::Create($node.Extent.Text))
}
$auth=Get-LocalMcpRuntimeAuthorization ($env:CATDESK_TEST_FORWARD -ceq '1')
$arguments=@('run','--config',$env:CATDESK_TEST_CONFIG)
try {
    $null=Invoke-BoundedTunnelClient -Path $env:CATDESK_TEST_CLIENT -CommandArgs $arguments -McpAuthorization $auth -TimeoutSeconds 7
    'RUNTIME_EXITED'
} catch {
    switch -Exact ($_.Exception.Message) {
        'tunnel-client command failed' { 'RUNTIME_EXITED_NONZERO' }
        'tunnel-client command exceeded bounded timeout and was killed' { 'RUNTIME_EXIT_PROVED_BY_OWNED_HANDLE' }
        default { exit 2 }
    }
}
"""
token = 'synthetic-loopback-proof-304782'
mcp_events = []
control_events = []
class Fixture(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass
    def handle_request(self):
        auth = self.headers.get('Authorization')
        is_control = self.server is control
        events = control_events if is_control else mcp_events
        n = int(self.headers.get('Content-Length','0'))
        method = None
        value = {}
        if n:
            data = self.rfile.read(n)
            if data.startswith(b'{'):
                value = json.loads(data)
                method = value.get('method')
        events.append((self.command,self.path.split('?')[0],method,auth == 'Bearer '+token))
        if is_control or not self.path.startswith('/mcp'):
            code, out = 404, None
        elif auth != 'Bearer '+token:
            code, out = 401, None
        elif self.command != 'POST':
            code, out = 405, None
        elif method == 'server/discover':
            code, out = 200, dict(jsonrpc='2.0', id=value['id'], error={'code':-32601,'message':'Method not found'})
        elif method == 'initialize':
            code, out = 200, dict(jsonrpc='2.0', id=value['id'], result=dict(protocolVersion='2025-03-26', capabilities={'tools': {}}, serverInfo={'name':'loopback-fixture','version':'1'}))
        else:
            code, out = 202, None
        data = json.dumps(out).encode() if out else b''
        self.send_response(code)
        if code == 401:
            self.send_header('WWW-Authenticate','Bearer')
        if data:
            self.send_header('Content-Type','application/json')
        self.send_header('Content-Length',str(len(data)))
        self.end_headers()
        if data:
            self.wfile.write(data)
    do_GET = do_POST = do_HEAD = handle_request

with tempfile.TemporaryDirectory(prefix='catdesk-native-mcp-loopback-') as folder:
    folder = Path(folder)
    with ThreadingHTTPServer(('127.0.0.1',0), Fixture) as mcp, ThreadingHTTPServer(('127.0.0.1',0), Fixture) as control:
        workers = [threading.Thread(target=server.serve_forever) for server in (mcp,control)]
        for worker in workers:
            worker.start()
        try:
            config = folder / 'loopback.json'
            config.write_text(json.dumps({
                'config_version': 1,
                'control_plane': {
                    'base_url': f'http://127.0.0.1:{control.server_port}',
                    'tunnel_id': 'tunnel_' + 32 * '1',
                    'api_key': 'env:CONTROL_PLANE_API_KEY',
                },
                'mcp': {
                    'startup_wait_timeout': '3s',
                    'server_urls': [{'channel':'main', 'url':f'http://127.0.0.1:{mcp.server_port}/mcp'}],
                },
                'admin_ui': {'open_browser':False},
                'health': {'listen_addr':'127.0.0.1:0', 'url_file':str(folder / 'health.url')},
                'log': {'file':str(folder / 'runtime.log'), 'level':'info', 'format':'json'},
            }), encoding='utf-8')
            env = {k:v for k,v in os.environ.items() if k.upper() in {'SYSTEMROOT','WINDIR','PATH','COMSPEC','PROGRAMFILES','PROGRAMFILES(X86)','PROGRAMDATA'}}
            env.update({k:str(folder) for k in ('HOME','USERPROFILE','LOCALAPPDATA','APPDATA','TEMP','TMP')})
            env.update(CATDESK_TEST_CLIENT=str(client),CATDESK_TEST_CONFIG=str(config),CONTROL_PLANE_API_KEY='synthetic-control-plane-loopback-only',CATDESK_MCP_AUTH_TOKEN=token)
            results=[]
            for forward in (False,True):
                mcp_events.clear()
                control_events.clear()
                env['CATDESK_TEST_FORWARD'] = '1' if forward else '0'
                log_file = folder / 'runtime.log'
                log_file.unlink(missing_ok=True)
                (folder / 'health.url').unlink(missing_ok=True)
                cmd = subprocess.run(['pwsh','-NoProfile','-NoLogo','-NonInteractive','-Command',ps],env=env,capture_output=True,text=True,timeout=25)
                expected = ('RUNTIME_EXITED','RUNTIME_EXITED_NONZERO','RUNTIME_EXIT_PROVED_BY_OWNED_HANDLE')
                result = cmd.stdout.strip()
                if result not in expected:
                    result = 'HARNESS_UNEXPECTED_OUTPUT'
                log_entries = [json.loads(line) for line in log_file.read_text(encoding='utf-8').splitlines()]
                results.append({
                    'forwarding': forward,
                    'processExit': cmd.returncode,
                    'runtime': result,
                    'mcpRequests': len(mcp_events),
                    'mcpAuthMatches': sum(match for _, _, _, match in mcp_events),
                    'initializeRequests': sum(method == 'initialize' for _, _, method, _ in mcp_events),
                    'initializeAuthMatches': sum(
                        method == 'initialize' and match for _, _, method, match in mcp_events
                    ),
                    'controlPlaneRequests': len(control_events),
                    'controlPlaneMcpBearerMatches': sum(match for _, _, _, match in control_events),
                    'runtimeHealthUrlPublished': (folder / 'health.url').exists(),
                    'sessionInitialized': any(entry.get('msg') == 'mcp session initialized' for entry in log_entries),
                })

        finally:
            for server in (mcp,control):
                server.shutdown()
            for worker in workers:
                worker.join(2)
            assert all(not w.is_alive() for w in workers)
baseline, forwarded = results
for result in results:
    if (result['processExit'] != 0 or result['runtime'] != 'RUNTIME_EXIT_PROVED_BY_OWNED_HANDLE'
            or result['controlPlaneRequests'] < 1 or result['controlPlaneMcpBearerMatches'] != 0
            or not result['runtimeHealthUrlPublished']):
        raise SystemExit('NATIVE_FIXTURE_OWNER_OR_CONTROL_PLANE_UNVERIFIED')
if (baseline['initializeRequests'] < 1 or baseline['mcpAuthMatches'] != 0
        or baseline['sessionInitialized']):
    raise SystemExit('NATIVE_FIXTURE_BASELINE_WRONG')
if (forwarded['initializeRequests'] < 1
        or forwarded['initializeAuthMatches'] != forwarded['initializeRequests']
        or forwarded['mcpAuthMatches'] != forwarded['mcpRequests']
        or not forwarded['sessionInitialized']):
    raise SystemExit('NATIVE_FIXTURE_FORWARDING_UNVERIFIED')
print(json.dumps({'state':'PASS','clientSha256':client_sha,'fixtureResults':results,'cleanupVerified':True}))
