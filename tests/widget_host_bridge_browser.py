"""Opt-in local SeleniumBase fixture; never opens ChatGPT or an authenticated profile.

Run with the existing qualified browser runtime and a fresh output directory.
The caller must provide its usual bounded whole-process-tree owner.
"""
import argparse
import hashlib
import json
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-directory", type=Path, required=True)
    args = parser.parse_args()
    output = args.output_directory.resolve()
    output.mkdir(parents=False, exist_ok=False)
    widget = (Path(__file__).resolve().parents[1] / "src/widget/catdesk_dashboard.html").read_text(encoding="utf-8")
    widget = widget.replace("__catdeskInitialTokenStatsLayoutPlaceholder__", "disable")
    usage = {"inputTokens": 0, "outputTokens": 0, "totalTokens": 0}
    payload = dict(schema="catdesk.review.v1", panelMode="tool_call", title="Local fixture", state="done",
                   toolName="fixture_read_only", tokenStatsLayout="disable", showDetailMode="expanded",
                   changedFiles=[], hasChanges=False, turnTokenUsage=usage, toolCallCount=0,
                   historyTurnTokenUsage=usage, historyToolCallCount=0, call="fixture-check", detail="BROWSER_FIXTURE_RENDERED")
    meta = {"catdesk/widgetPayload": payload}
    legacy = widget.replace("<script>", "<script>window.openai=" + json.dumps({"toolResponseMetadata": {"_meta": meta}}) + ";", 1)
    encode = lambda value: json.dumps(value).replace("<", "\\u003c")
    host = """<!doctype html><meta charset="utf-8"><title>Local widget host fixture</title>
<iframe id="standard"></iframe><iframe id="legacy"></iframe><iframe id="refused"></iframe>
<script>
const META=__META__;
const counts={standard:0,legacy:0,refused:0};
let pong=false,teardown=false;
window.addEventListener('message', event=>{
 const id=Object.keys(counts).find(id=>document.getElementById(id).contentWindow===event.source);
 if(!id||!event.data||event.data.jsonrpc!=='2.0')return;
 const m=event.data;
 if(m.id==='fixture-ping'&&m.result)pong=true;
 if(m.id==='fixture-teardown'&&m.result){teardown=true;event.source.postMessage({jsonrpc:'2.0',method:'ui/notifications/tool-result',params:{_meta:{'catdesk/widgetPayload':{...META['catdesk/widgetPayload'],detail:'LATE_AFTER_TEARDOWN'}}}},'*');}
 if(m.method==='ui/initialize'){
  counts[id]++;
  if(id==='legacy')return;
  event.source.postMessage(id==='refused'?{jsonrpc:'2.0',id:m.id,error:{code:-32601,message:'PRIVATE_FIXTURE_ERROR'}}:
   {jsonrpc:'2.0',id:m.id,result:{protocolVersion:'2026-01-26',hostInfo:{name:'Local fixture',version:'1'},hostCapabilities:{}}},'*');
 }
 if(m.method==='ui/notifications/initialized'){event.source.postMessage({jsonrpc:'2.0',method:'ui/notifications/tool-result',params:{_meta:META}},'*');if(id==='standard')event.source.postMessage({jsonrpc:'2.0',id:'fixture-ping',method:'ping'},'*');}
 if(m.method==='ui/notifications/size-changed'&&Number.isFinite(m.params?.height))document.getElementById(id).style.height=m.params.height+'px';
});
window.fixtureStatus=()=>{
 const text=id=>document.getElementById(id).contentDocument?.getElementById('root')?.textContent||'';
 return {standardRendered:text('standard').includes('BROWSER_FIXTURE_RENDERED'),
  legacyRendered:text('legacy').includes('BROWSER_FIXTURE_RENDERED'),
  refusalVisible:text('refused').includes('UI Host Unavailable'),
  standardHeight:document.getElementById('standard').getBoundingClientRect().height,
  standardFits:document.getElementById('standard').clientHeight>=Math.ceil(document.getElementById('standard').contentDocument.getElementById('root').getBoundingClientRect().height+24),
  privateErrorVisible:text('refused').includes('PRIVATE_FIXTURE_ERROR'),pong,teardown,lateResultVisible:text('standard').includes('LATE_AFTER_TEARDOWN'),counts};
};
document.getElementById('standard').srcdoc=WIDGET;
document.getElementById('refused').srcdoc=WIDGET;
document.getElementById('legacy').srcdoc=LEGACY;
</script>"""
    host = host.replace("__META__", encode(meta)).replace("srcdoc=WIDGET", "srcdoc=" + encode(widget)).replace("srcdoc=LEGACY", "srcdoc=" + encode(legacy))
    fixture = output / "host.html"
    fixture.write_text(host, encoding="utf-8")
    from seleniumbase import sb_cdp
    started = time.monotonic()
    sb = None
    try:
        sb = sb_cdp.Chrome(fixture.as_uri(), headless=True,
                           user_data_dir=str(output / "disposable-browser-state"))
        state = None
        while time.monotonic() - started < 25:
            state = sb.evaluate("window.fixtureStatus ? window.fixtureStatus() : null")
            if state and state["standardRendered"] and state["legacyRendered"] and state["refusalVisible"]:
                break
            time.sleep(0.1)
        if not state or not all(state.get(k) for k in ("standardRendered", "legacyRendered", "refusalVisible")):
            raise RuntimeError("LOCAL_WIDGET_RENDER_NOT_QUALIFIED")
        # The legacy result must survive the real five-second standard-bridge timeout.
        time.sleep(max(0, 6 - (time.monotonic() - started)))
        state = sb.evaluate("window.fixtureStatus()")
        if state["privateErrorVisible"] or not state["legacyRendered"] or not state["standardFits"] or not state["pong"] or any(v != 1 for v in state["counts"].values()):
            raise RuntimeError("LOCAL_WIDGET_BRIDGE_BOUNDARY_FAILED")
        sb.evaluate("document.getElementById('standard').contentWindow.postMessage({jsonrpc:'2.0',id:'fixture-teardown',method:'ui/resource-teardown',params:{reason:'local fixture complete'}},'*')")
        time.sleep(0.2)
        state = sb.evaluate("window.fixtureStatus()")
        if not state["teardown"] or state["lateResultVisible"]:
            raise RuntimeError("LOCAL_WIDGET_TEARDOWN_NOT_QUALIFIED")
        sb.save_screenshot(str(output / "fixture.png"))
    finally:
        if sb is not None:
            sb.quit()
    result = {"scope": "LOCAL_HEADLESS_BROWSER_FIXTURE_NOT_CHATGPT_HOST_ACCEPTANCE",
              "passed": True, "elapsedSeconds": round(time.monotonic() - started, 3), "state": state,
              "widgetSha256": hashlib.sha256(widget.encode()).hexdigest(),
              "chatgptRequests": 0, "mcpToolExecutions": 0, "authenticatedProfileUsed": False,
              "browserContextExited": True}
    (output / "result.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result), flush=True)


if __name__ == "__main__":
    main()
