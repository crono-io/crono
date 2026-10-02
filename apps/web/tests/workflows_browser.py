"""Exercise built Workflow pages against a controlled HTTP API using ChromeDriver.

Run after `trunk build --release`: python3 apps/web/tests/workflows_browser.py.
The isolated fixture serves only dist assets and public DTOs. It changes no real
Crono data, uses no credentials, and controls failures/progress to check retries,
terminal polling cleanup, immutable views, and responsive keyboard operation.
It also measures shared field alignment and height across resource forms and
Run filters; set CRONO_WEB_FORMS_ONLY=1 to run those checks independently.
Structured-input checks verify blur, preview and submission normalization while
preserving script, argument and JSON whitespace; CRONO_WEB_INPUTS_ONLY=1 isolates
those checks. HTTP fixtures echo submitted Jobs without normalizing them.
"""

import base64
import copy
import json
import mimetypes
import os
import subprocess
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from uuid import UUID

DIST = Path(os.environ.get("CRONO_WEB_DIST", str(Path(__file__).resolve().parents[1] / "dist")))
STAMP = "2026-10-02T12:00:00Z"
def identity(number):
    return str(UUID(int=number))

NS, OTHER, WORKFLOW, TARGET, TARGET_SET, INVOCATION = map(identity, range(1, 7))
NAMES = ["backup", "upgrade", "validate", "rollback"]
def definition(name="postgres-upgrade"):
    return dict(id=WORKFLOW, namespace_id=NS, namespace="operations", name=name,
                description="Back up, upgrade, validate or roll back", revision=1,
                nodes=[dict(id=identity(20+i), name=node, job_id=identity(10+i)) for i, node in enumerate(NAMES)],
                edges=[],
                created_at=STAMP, updated_at=STAMP)

WORKFLOW_DATA = definition()
WORKFLOW_DATA["edges"] = [{"from": "backup", "to": "upgrade", "condition": "success"},
                          {"from": "upgrade", "to": "validate", "condition": "success"},
                          {"from": "upgrade", "to": "rollback", "condition": "failure"}]
STATE = dict(workflow=copy.deepcopy(WORKFLOW_DATA), fail_list=False, fail_launch=True,
             requests=[], launch_requests=[], job_writes=[], polls=0, active=0, max_active=0,
             permanent_running=False, history=True, poll_failure=False, cancelled=False, launch_delay=0, fail_workflow_get=False)

def invocation(terminal=False):
    states = ["succeeded", "succeeded", "succeeded", "skipped"] if terminal else ["succeeded", "running", "pending", "pending"]
    nodes = [dict(id=identity(30+i), workflow_node_id=identity(20+i), name=name,
                  job_id=identity(10+i), state=state,
                  runs=[] if state in ("pending", "skipped") else
                  [dict(target_id=TARGET, run_id=identity(40+i)), dict(target_id=identity(7), run_id=identity(50+i))],
                  started_at=None if state in ("pending", "skipped") else STAMP,
                  finished_at=STAMP if state in ("succeeded", "skipped") else None)
             for i, (name, state) in enumerate(zip(NAMES, states))]
    return dict(id=INVOCATION, request_id=identity(60), workflow=copy.deepcopy(WORKFLOW_DATA),
                target=dict(kind="target_set", id=TARGET_SET), inputs={"version":"17"},
                state="succeeded" if terminal else "running", cancellation_requested=False,
                nodes=nodes, created_at=STAMP, started_at=STAMP,
                finished_at=STAMP if terminal else None)

def page(items):
    return dict(items=items, next_cursor=None)

def error(message, code="fixture_failure"):
    return dict(error=dict(code=code, message=message, field=None))

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def respond(self, code, value):
        content = json.dumps(value).encode() if value is not None else b""
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        try:
            self.wfile.write(content)
        except BrokenPipeError:
            pass  # Leaving a route can cancel its in-flight status request.

    def do_GET(self):
        path = urllib.parse.urlparse(self.path).path
        STATE["requests"].append(("GET", self.path))
        if path == "/api/namespaces":
            self.respond(200, page([dict(id=NS, name="operations", created_at=STAMP), dict(id=OTHER, name="other", created_at=STAMP)]))
        elif path == "/api/queues":
            self.respond(200, page([dict(id=identity(8), name="default", description=None,
                                       enabled=True, system=True, created_at=STAMP, updated_at=STAMP)]))
        elif path.startswith("/api/") and path.endswith("/jobs"):
            jobs = [dict(id=identity(10+i), namespace_id=NS, namespace="operations", name=name,
                         qualified_name=f"operations/{name}", executor="process", queue_id=identity(8), queue="default",
                         executable="/bin/true", arguments=[], inputs={}, idempotent=True, dry_run=False,
                         max_attempts=1, retry_initial_seconds=1, retry_max_seconds=10, retry_multiplier=2.0,
                         retry_jitter=0.2, created_at=STAMP, updated_at=STAMP) for i, name in enumerate(NAMES)]
            self.respond(200, page(jobs if NS in path else []))
        elif path.startswith("/api/") and path.endswith("/targets"):
            self.respond(200, page([dict(id=TARGET, namespace_id=NS, namespace="operations", name="db-1", qualified_name="operations/db-1", arguments=[], inputs={}, created_at=STAMP, updated_at=STAMP)]))
        elif path.startswith("/api/") and path.endswith("/target-sets"):
            self.respond(200, page([dict(id=TARGET_SET, namespace_id=NS, namespace="operations", name="database-fleet", qualified_name="operations/database-fleet", targets=[], inputs={}, created_at=STAMP, updated_at=STAMP)]))
        elif path.startswith("/api/") and path.endswith("/workflows"):
            if STATE["fail_list"]:
                self.respond(503, error("Workflow catalog temporarily unavailable"))
            else:
                self.respond(200, page([STATE["workflow"]] if NS in path and STATE["workflow"] else []))
        elif path == f"/api/workflows/{WORKFLOW}":
            if STATE["fail_workflow_get"]:
                self.respond(503, error("Definition temporarily unavailable"))
            else:
                self.respond(200, STATE["workflow"])
        elif path == f"/api/workflows/{WORKFLOW}/runs":
            self.respond(200, page([invocation(True)] if STATE["history"] else []))
        elif path == f"/api/workflow-runs/{INVOCATION}":
            STATE["polls"] += 1
            STATE["active"] += 1
            STATE["max_active"] = max(STATE["max_active"], STATE["active"])
            time.sleep(.15)
            STATE["active"] -= 1
            if STATE["poll_failure"]:
                self.respond(503, error("Workflow status temporarily unavailable"))
            else:
                result = invocation(STATE["cancelled"] or (STATE["polls"] >= 3 and not STATE["permanent_running"]))
                if STATE["cancelled"]: result.update(state="cancelled", cancellation_requested=True)
                self.respond(200, result)
        elif path.startswith("/api/"):
            self.respond(200, page([]))
        else:
            candidate = (DIST / path.lstrip("/")).resolve()
            if not candidate.is_relative_to(DIST.resolve()):
                self.respond(404, error("Not found"))
                return
            if not candidate.is_file():
                candidate = DIST / "index.html"
            content = candidate.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", mimetypes.guess_type(candidate)[0] or "application/octet-stream")
            self.send_header("Content-Length", str(len(content)))
            self.end_headers()
            self.wfile.write(content)

    def do_POST(self):
        value = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))) or b"{}")
        STATE["requests"].append(("POST", self.path))
        if self.path == f"/api/namespaces/{NS}/jobs":
            STATE["job_writes"].append(value)
            self.respond(201, dict(value, id=identity(77), namespace_id=NS,
                                   namespace="operations", qualified_name=f"operations/{value['name']}",
                                   queue="default", created_at=STAMP, updated_at=STAMP))
        elif self.path == f"/api/workflows/{WORKFLOW}/runs":
            STATE["launch_requests"].append(value)
            time.sleep(STATE["launch_delay"])
            if STATE["fail_launch"]:
                STATE["fail_launch"] = False
                self.respond(503, error("Launch response lost; retry the unchanged request"))
            else:
                self.respond(201, invocation())
        elif self.path == f"/api/workflow-runs/{INVOCATION}/cancel":
            result = invocation(True)
            result.update(state="cancelled", cancellation_requested=True)
            STATE["cancelled"] = True
            self.respond(200, result)
        else:
            graph = copy.deepcopy(STATE["workflow"] or WORKFLOW_DATA)
            graph.update(value)
            graph["nodes"] = [dict(id=identity(100+i), **node) for i, node in enumerate(value["nodes"])]
            STATE["workflow"] = graph
            self.respond(201, graph)

    def do_PUT(self):
        value = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        STATE["requests"].append(("PUT", self.path))
        # The fixture rejects a known cyclic edit; real backend tests own DAG validation.
        if any(edge["to"] == "backup" for edge in value["edges"]):
            self.respond(400, error("Workflow contains a cycle; remove a dependency"))
        elif value["revision"] != STATE["workflow"]["revision"]:
            self.respond(409, error("resource already exists", "already_exists"))
        else:
            graph = copy.deepcopy(STATE["workflow"])
            graph.update(value)
            graph["nodes"] = [dict(id=identity(100+i), **node) for i, node in enumerate(value["nodes"])]
            graph["revision"] += 1
            STATE["workflow"] = graph
            self.respond(200, graph)

    def do_DELETE(self):
        STATE["requests"].append(("DELETE", self.path))
        if STATE["history"]:
            self.respond(409, error("Workflow with execution history cannot be deleted"))
        else:
            STATE["workflow"] = None
            self.respond(204, None)

OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))
def request(url, method="GET", data=None):
    body = None if data is None else json.dumps(data).encode()
    with OPENER.open(urllib.request.Request(url, body, {"Content-Type":"application/json"}, method=method), timeout=30) as response:
        return json.loads(response.read())["value"]

def main():
    assert (DIST / "index.html").is_file(), "Run trunk build --release first"
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    driver = subprocess.Popen([os.environ.get("CHROMEDRIVER", "chromedriver"), "--port=0"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    # ChromeDriver chooses its port and prints it before accepting sessions.
    port = None
    for line in driver.stdout:
        if "successfully on port" in line:
            port = int(line.split("port ")[1].rstrip(".\n"))
            break
    assert port, "ChromeDriver did not start"
    base = f"http://127.0.0.1:{port}"
    session = None
    try:
        session = request(base+"/session", "POST", {"capabilities":{"alwaysMatch":{"browserName":"chrome", "goog:chromeOptions":{"args":["--headless=new","--no-sandbox","--no-proxy-server","--disable-dev-shm-usage","--window-size=1440,1000"]}, "goog:loggingPrefs":{"browser":"ALL"}}}})["sessionId"]
        base += f"/session/{session}"
        def js(script, *args):
            return request(base+"/execute/sync", "POST", {"script":script,"args":list(args)})
        def wait(predicate, message, timeout=15):
            deadline = time.monotonic()+timeout
            while time.monotonic() < deadline:
                if predicate(): return
                time.sleep(.05)
            raise AssertionError(message+"\n"+js("return document.body.innerText")+"\n"+str(request(base+"/log", "POST", {"type":"browser"}))+"\nRequests: "+str(STATE["requests"][-10:]))
        def text(): return js("return document.body.innerText")
        def open_page(path):
            request(base+"/url", "POST", {"url":f"http://127.0.0.1:{server.server_port}{path}"})
            wait(lambda: js("return !!document.querySelector('main')"), "App did not mount")
        def click(label):
            assert js("const b=[...document.querySelectorAll('button')].find(b=>{const c=b.cloneNode(true); c.querySelectorAll('[aria-hidden=true]').forEach(e=>e.remove()); const modal=document.querySelector('dialog[open]'); return b.offsetParent && (!modal || modal.contains(b)) && c.textContent.trim()===arguments[0]}); if(!b) return false; b.click(); return true", label), f"Button missing: {label}"
        def fill(selector, value):
            assert js("const e=document.querySelector(arguments[0]); if(!e) return false; e.value=arguments[1]; e.dispatchEvent(new Event('input',{bubbles:true})); return true",selector,value), selector
        def choose(selector, label):
            wait(lambda: js("const e=document.querySelector(arguments[0]); return e && !e.disabled",selector), "Selector unavailable")
            js("document.querySelector(arguments[0]).focus()", selector)
            wait(lambda: js("return [...document.querySelectorAll('[role=option] button')].some(b=>{const c=b.cloneNode(true); c.querySelectorAll('[aria-hidden=true]').forEach(e=>e.remove()); return c.textContent.trim()===arguments[0]})",label), "Option missing")
            js("const b=[...document.querySelectorAll('[role=option] button')].find(b=>{const c=b.cloneNode(true); c.querySelectorAll('[aria-hidden=true]').forEach(e=>e.remove()); return c.textContent.trim()===arguments[0]}); b.dispatchEvent(new MouseEvent('mousedown',{bubbles:true,cancelable:true})); document.querySelector(arguments[1]).blur()",label,selector)
        def structured_inputs_check():
            def blur(selector):
                js("document.querySelector(arguments[0]).dispatchEvent(new Event('blur'))", selector)
            def executor(kind):
                js("const e=document.querySelector('form label select'); e.value=arguments[0]; e.dispatchEvent(new Event('change',{bubbles:true}))", kind)
            open_page("/jobs/new")
            choose("#job-namespace", "operations")
            choose("#job-queue", "default")
            executor("process")
            choose("#job-preview-target", "Target · db-1")
            fill("#job-name", " \techo\u00a0")
            executable = "input[placeholder='/usr/bin/echo']"
            fill(executable, " /usr/bin/echo \t")
            assert js("return document.querySelector('#job-name').value") == " \techo\u00a0", "Name changed while typing"
            assert js("return document.querySelector(arguments[0]).value", executable) == " /usr/bin/echo \t", "Path changed while typing"
            wait(lambda: '$ "/usr/bin/echo"' in js("return document.querySelector('form pre').textContent"), "Preview did not use the trimmed path")
            click("+ Add argument")
            fill("#job-arguments-0", " literal ")
            fill("#job-inputs", '{"message":" value "}')
            blur("#job-name")
            blur(executable)
            blur("#job-arguments-0")
            blur("#job-inputs")
            wait(lambda: js("return document.querySelector('#job-name').value") == "echo", "Name not trimmed on blur")
            assert js("return document.querySelector(arguments[0]).value", executable) == "/usr/bin/echo", "Executable not trimmed on blur"
            assert js("return document.querySelector('#job-arguments-0').value") == " literal ", "Argument changed on blur"
            js("document.querySelector('#job-name').scrollIntoView({block:'center'})")
            Path("/tmp/crono-input-normalization.png").write_bytes(base64.b64decode(request(base+"/screenshot")))
            # Submit padded fields without blurring them: keyboard/programmatic submission
            # must produce the same request as mouse submission after leaving a field.
            fill("#job-name", " echo ")
            fill(executable, " /usr/bin/echo \u0085")
            click("Save Job")
            wait(lambda: "Saved operations/echo." in text(), "Padded Job did not save")
            submitted = STATE["job_writes"][-1]
            assert submitted["name"] == "echo" and submitted["executable"] == "/usr/bin/echo", submitted
            assert submitted["arguments"] == [" literal "] and submitted["inputs"] == {"message":" value "}, submitted
            open_page("/jobs/new")
            choose("#job-namespace", "operations")
            executor("shell")
            script = "\n  printf '%s' \"$1\"  \n"
            fill("#job-name", " shell ")
            fill("input[placeholder='/bin/sh']", " /bin/sh ")
            fill("textarea[placeholder^=\"printf\"]", script)
            blur("textarea[placeholder^=\"printf\"]")
            click("Save Job")
            wait(lambda: "Saved operations/shell." in text(), "Shell Job did not save")
            assert STATE["job_writes"][-1]["executable"] == "/bin/sh"
            assert STATE["job_writes"][-1]["shell_command"] == script, "Script whitespace changed"
            open_page("/schedules")
            wait(lambda: js("return !!document.querySelector('#schedule-timezone')"), "Schedule form missing")
            cron = "form label input:not([id])"
            fill(cron, " \t0  0 * * * ")
            blur(cron)
            wait(lambda: js("return document.querySelector(arguments[0]).value", cron) == "0  0 * * *", "Cron padding not trimmed or internal whitespace changed")
            js("const e=document.querySelector('form label select'); e.value='once'; e.dispatchEvent(new Event('change',{bubbles:true}))")
            once = "input[placeholder='2026-09-25T12:00:00Z']"
            wait(lambda: js("return !!document.querySelector(arguments[0])", once), "One-shot field missing")
            fill(once, " 2030-01-01T00:00:00Z ")
            blur(once)
            wait(lambda: js("return document.querySelector(arguments[0]).value", once) == "2030-01-01T00:00:00Z", "Timestamp padding not trimmed")
            print("Structured input browser checks passed: typing preserved, blur/save/preview agree, Unicode padding removed, arguments/JSON/scripts unchanged, cron and timestamp blur")
        def form_controls_check():
            def aligned(selectors):
                boxes = js("return arguments[0].map(s=>{const e=document.querySelector(s); const r=e.getBoundingClientRect(); return {selector:s,top:r.top,height:r.height}})", selectors)
                assert max(box["top"] for box in boxes)-min(box["top"] for box in boxes) < 1, f"Field tops differ: {boxes}"
                assert all(abs(box["height"]-44) < 1 for box in boxes), f"Controls must share a 44 px height: {boxes}"
            open_page("/jobs/new")
            wait(lambda: js("return !!document.querySelector('#job-queue')"), "Job form missing")
            aligned(["#job-name", "#job-queue"])
            aligned(["label select", "input[placeholder='/usr/bin/echo']"])
            assert js("const e=document.querySelector('#job-queue'); const a=e.parentElement.querySelector('[aria-hidden=true]'); const r=e.getBoundingClientRect(),s=a.getBoundingClientRect(); return Math.abs(r.top+r.height/2-s.top-s.height/2)<1"), "Queue arrow is not centered"
            choose("#job-namespace", "operations")
            choose("#job-queue", "default")
            js("document.querySelector('#job-name').focus()")
            request(base+"/actions", "POST", {"actions":[{"type":"key","id":"keyboard","actions":[{"type":"keyDown","value":"\ue004"},{"type":"keyUp","value":"\ue004"}]}]})
            assert js("return document.activeElement.id") == "job-queue", "Name and Queue tab order changed"
            request(base+"/actions", "POST", {"actions":[{"type":"key","id":"keyboard","actions":[{"type":"keyDown","value":"\ue00c"},{"type":"keyUp","value":"\ue00c"}]}]})
            wait(lambda: js("return document.querySelector('#job-queue').getAttribute('aria-expanded')==='false'"), "Escape did not close the Queue options")
            request(base+"/window/rect", "POST", {"width":390,"height":844})
            js("document.querySelector('#job-name').scrollIntoView({block:'center'})")
            assert js("return document.documentElement.scrollWidth <= innerWidth"), "Job form widened mobile viewport"
            assert js("return document.querySelector('#job-queue').getBoundingClientRect().top > document.querySelector('#job-name').getBoundingClientRect().top"), "Mobile fields must stack"
            Path("/tmp/crono-job-form-mobile.png").write_bytes(base64.b64decode(request(base+"/screenshot")))
            request(base+"/window/rect", "POST", {"width":1440,"height":1000})
            js("document.querySelector('#job-name').scrollIntoView({block:'center'})")
            Path("/tmp/crono-job-form-desktop.png").write_bytes(base64.b64decode(request(base+"/screenshot")))
            open_page("/runs")
            wait(lambda: js("return !!document.querySelector('#runs-status-filter')"), "Run filters missing")
            aligned(["#runs-namespace-filter", "#runs-status-filter", "#runs-job-filter", "#runs-target-filter", "#runs-target-set-filter"])
            js("document.querySelector('#runs-status-filter').focus()")
            assert js("const s=getComputedStyle(document.activeElement); return s.outlineStyle !== 'none' || s.boxShadow !== 'none'"), "Status filter has no keyboard focus indication"
            request(base+"/goog/cdp/execute", "POST", {"cmd":"Emulation.setEmulatedMedia", "params":{"features":[{"name":"forced-colors","value":"active"}]}})
            assert js("const s=getComputedStyle(document.activeElement); return matchMedia('(forced-colors:active)').matches && s.outlineStyle==='solid' && parseFloat(s.outlineWidth)>=2 && s.appearance==='auto'"), "High-contrast mode lost native arrow or focus outline"
            request(base+"/goog/cdp/execute", "POST", {"cmd":"Emulation.setEmulatedMedia", "params":{"features":[]}})
            js("document.querySelector('#runs-status-filter').value='failed'; document.querySelector('#runs-status-filter').dispatchEvent(new Event('change',{bubbles:true}))")
            wait(lambda: any(method == "GET" and "status=failed" in path for method, path in STATE["requests"]), "Status filter no longer reaches API")
            js("document.querySelector('#runs-status-filter').scrollIntoView({block:'center'})")
            Path("/tmp/crono-run-filters-desktop.png").write_bytes(base64.b64decode(request(base+"/screenshot")))
            for path in ("/targets/new", "/runs/new", "/namespaces", "/queues", "/target-sets", "/schedules", "/workflows/new", f"/workflows/{WORKFLOW}/edit"):
                open_page(path)
                wait(lambda: js("return !!document.querySelector('main input')"), f"Form missing on {path}")
                if path.endswith("/edit"):
                    wait(lambda: len(js("return [...document.querySelectorAll('input[id^=workflow-node-name-]')]")) == 4, "Workflow editor did not load")
                    aligned(["input[id^=workflow-node-name-]", "input[id^=workflow-node-job-]"])
                    aligned(["input[id^=workflow-edge-from-]", "select[id^=workflow-edge-condition-]", "input[id^=workflow-edge-to-]"])
                    assert js("return document.querySelector('.crono-readonly-field').getBoundingClientRect().height===44"), "Fixed Namespace height differs from editable fields"
                if path == "/schedules":
                    aligned(["label select", "label input", "#schedule-timezone"])
                measurements = js("return [...document.querySelectorAll('main input:not([type=checkbox]),main select')].map(e=>({id:e.id,height:e.getBoundingClientRect().height}))")
                assert all(abs(item["height"]-44) < 1 for item in measurements), f"Inconsistent controls on {path}: {measurements}"
                request(base+"/window/rect", "POST", {"width":390,"height":844})
                assert js("return document.documentElement.scrollWidth <= innerWidth"), f"Form overflow on {path}"
                request(base+"/window/rect", "POST", {"width":1440,"height":1000})
            print("Form browser checks passed: aligned Job/Queue, equal-height Run filters, shared controls across all resource forms, keyboard focus/tab order, API status filtering, mobile stacking/overflow")
        def late_launch_check():
            STATE.update(workflow=copy.deepcopy(WORKFLOW_DATA), fail_launch=False, launch_delay=1.2)
            open_page(f"/workflows/{WORKFLOW}/run")
            choose("#workflow-run-destination", "Target · db-1")
            click("Run Workflow")
            js("document.querySelector('a[href=\"/workflows\"]').click()")
            wait(lambda: js("return location.pathname") == "/workflows", "Did not leave launch page")
            time.sleep(1.5)
            assert js("return location.pathname") == "/workflows", "Late launch response hijacked navigation"
            STATE["launch_delay"] = 0
        def reload_failure_check():
            STATE.update(workflow=copy.deepcopy(WORKFLOW_DATA), fail_workflow_get=False)
            open_page(f"/workflows/{WORKFLOW}/edit")
            wait(lambda: js("return !!document.querySelector('#workflow-description')"), "Editor did not mount")
            fill("#workflow-description", "Preserve draft through reload failure")
            click("Reload latest")
            STATE["fail_workflow_get"] = True
            click("Discard draft and reload")
            wait(lambda: "Definition temporarily unavailable" in text(), "Reload failure hidden")
            assert js("return document.querySelector('#workflow-description')?.value") == "Preserve draft through reload failure", "Reload failure discarded the draft"
            STATE["fail_workflow_get"] = False
            click("Keep draft")
            fill("input[id^=workflow-node-name-]", "edited-backup")
            click("Reload latest")
            click("Discard draft and reload")
            wait(lambda: js("return document.querySelector('#workflow-description')?.value") == WORKFLOW_DATA["description"], "Reload did not restore metadata")
            assert js("return document.querySelector('input[id^=workflow-node-name-]').value") == "backup", "Reloaded graph and visible node fields disagree"
        if os.environ.get("CRONO_WEB_INPUTS_ONLY") == "1":
            structured_inputs_check()
            return
        if os.environ.get("CRONO_WEB_FORMS_ONLY") == "1":
            form_controls_check()
            return
        if os.environ.get("CRONO_WORKFLOW_RELOAD_FAILURE_ONLY") == "1":
            reload_failure_check()
            print("Reload failure preserves draft")
            return
        if os.environ.get("CRONO_WORKFLOW_LATE_LAUNCH_ONLY") == "1":
            late_launch_check()
            print("Late launch stays on the route chosen by the operator")
            return
        open_page("/workflows")
        choose("#workflows-namespace-filter","operations")
        wait(lambda: "postgres-upgrade" in text(), "Workflow not listed")
        assert "All Workflows" in text() and "Create Workflow" in text()
        STATE["fail_list"] = True
        click("Refresh")
        wait(lambda: "Workflow catalog temporarily unavailable" in text(), "API list error hidden")
        STATE["fail_list"] = False
        click("Retry")
        wait(lambda: "postgres-upgrade" in text(), "Retry did not restore list")
        choose("#workflows-namespace-filter","other")
        wait(lambda: "No workflows yet" in text(), "Namespace empty state missing")

        open_page(f"/workflows/{WORKFLOW}")
        wait(lambda: "Recent Workflow runs" in text() and "Open WorkflowRun" in text(), "Workflow details/history did not load")
        assert len(js("return [...document.querySelectorAll('[data-workflow-node]')]")) == 4
        open_page(f"/workflows/{WORKFLOW}/edit")
        wait(lambda: len(js("return [...document.querySelectorAll('input[id^=workflow-node-name-]')]")) == 4, "Editor did not restore nodes")
        assert "Changes affect future workflow runs only" in text()
        click("+ Add Dependency")
        rows = js("return [...document.querySelectorAll('input[id^=workflow-edge-from-]')].map(e=>e.id)")
        from_id = rows[-1]
        to_id = from_id.replace("-from-", "-to-")
        choose("#"+from_id,"rollback")
        choose("#"+to_id,"backup")
        click("Save Workflow")
        wait(lambda: js("return !!document.querySelector('dialog[open]')"), "Validation feedback dialog missing")
        assert "Workflow contains a cycle" in text()
        click("Back to form")
        assert js("return [...document.querySelectorAll('form [role=alert]')].some(e=>e.textContent.includes('contains a cycle'))"), "Cycle error missing near editor"
        assert len(js("return [...document.querySelectorAll('input[id^=workflow-node-name-]')]")) == 4, "Draft lost on error"
        js("const buttons=[...document.querySelectorAll('button[aria-label=\"Remove dependency\"]')]; buttons.at(-1).click()")
        fill("#workflow-description", "Future runs only")
        STATE["workflow"]["revision"] = 2  # Another operator saved after this page loaded.
        STATE["workflow"]["nodes"][0]["job_id"] = identity(99)  # Reference absent from this tab's Job catalog.
        click("Save Workflow")
        wait(lambda: "use Reload latest" in text(), "Revision conflict lacks recovery guidance")
        click("Back to form")
        click("Reload latest")
        wait(lambda: "Copy current draft" in text(), "Reload confirmation missing")
        assert "Future runs only" in js("return document.querySelector('#workflow-reload-latest pre').textContent")
        click("Keep draft")
        assert js("return document.querySelector('#workflow-description').value") == "Future runs only"
        click("Reload latest")
        click("Discard draft and reload")
        wait(lambda: js("return document.querySelector('#workflow-description')?.value") == WORKFLOW_DATA["description"], "Latest revision did not replace the confirmed draft")
        fill("#workflow-description", "Future runs only")
        click("Save Workflow")
        wait(lambda: "Saved Workflow" in text(), "Workflow update failed")
        click("Keep editing")
        assert STATE["workflow"]["revision"] == 3
        assert STATE["workflow"]["nodes"][0]["job_id"] == identity(99), "Reload silently dropped a current Job reference"
        node = js("return document.querySelector('input[id^=workflow-node-name-]').id")
        js("document.querySelector(arguments[0]).closest('section > div').querySelector('button[aria-label^=\"Remove Job\"]').click()", "#"+node)
        wait(lambda: "Removing backup also removes" in text(), "Connected node removal not confirmed")
        click("Remove Job")
        wait(lambda: len(js("return [...document.querySelectorAll('input[id^=workflow-node-name-]')]")) == 3, "Node not removed")
        assert len(js("return [...document.querySelectorAll('input[id^=workflow-edge-from-]')]")) == 2

        open_page("/workflows/new")
        choose("#workflow-namespace", "operations")
        choose("input[id^=workflow-node-job-]", "backup")
        fill("#workflow-name", "single-backup")
        click("+ Add Job")
        assert len(js("return [...document.querySelectorAll('input[id^=workflow-node-name-]')]")) == 2
        js("document.querySelectorAll('button[aria-label^=\"Remove Job\"]')[1].click()")
        choose("#workflow-namespace", "other")
        wait(lambda: js("return document.querySelector('input[id^=workflow-node-job-]').value===''"), "Namespace retained incompatible Job")
        choose("#workflow-namespace", "operations")
        choose("input[id^=workflow-node-job-]", "backup")
        click("Save Workflow")
        wait(lambda: "Saved Workflow single-backup" in text(), "Workflow creation failed")
        STATE["workflow"] = copy.deepcopy(WORKFLOW_DATA)

        open_page(f"/workflows/{WORKFLOW}/run")
        choose("#workflow-run-destination", "Target Set · database-fleet")
        fill("#workflow-run-inputs", '{"version":"17"}')
        assert "Jobs: 4" in text() and "Target Set · database-fleet" in text()
        click("Run Workflow")
        wait(lambda: "Launch response lost" in text(), "Launch failure hidden")
        click("Run Workflow")
        wait(lambda: js("return location.pathname").startswith("/workflow-runs/"), "Launch did not open invocation")
        wait(lambda: "Execution progress" in text(), "Invocation not loaded")
        assert len(STATE["launch_requests"]) == 2 and STATE["launch_requests"][0] == STATE["launch_requests"][1], "Retry duplicated launch identity"
        wait(lambda: "Automatic refresh has stopped" in text(), "Polling did not reach terminal state", 20)
        assert "Expected branch skip: upgrade ended succeeded" in text()
        assert js("return document.querySelectorAll('table a[href^=\"/runs/\"]').length") == 6, "Target Set child Run links missing"
        assert STATE["max_active"] == 1, "Overlapping polling requests"
        polls = STATE["polls"]
        time.sleep(5.5)
        assert STATE["polls"] == polls, "Polling continued after terminal state"
        request(base+"/window/rect", "POST", {"width":390,"height":844})
        assert js("return document.documentElement.scrollWidth <= innerWidth"), "Graph widened viewport"
        js("document.querySelector('main').scrollIntoView({block:'start'})")
        Path("/tmp/crono-workflow-mobile.png").write_bytes(base64.b64decode(request(base+"/screenshot")))
        request(base+"/window/rect", "POST", {"width":1440,"height":1000})
        js("document.querySelector('figure').scrollIntoView({block:'start'})")
        Path("/tmp/crono-workflow-desktop.png").write_bytes(base64.b64decode(request(base+"/screenshot")))

        STATE.update(polls=0, permanent_running=True)
        open_page(f"/workflow-runs/{INVOCATION}")
        wait(lambda: "Execution progress" in text(), "Active invocation did not reload")
        STATE["poll_failure"] = True
        click("Refresh")
        wait(lambda: "Displaying the last successful snapshot" in text(), "Refresh failure falsely showed current data")
        STATE["poll_failure"] = False
        click("Refresh")
        wait(lambda: "Displaying the last successful snapshot" not in text(), "Refresh did not recover")
        click("Cancel Workflow run")
        wait(lambda: js("return document.activeElement.textContent==='Keep running'"), "Cancel did not focus safe action")
        click("Keep running")
        click("Cancel Workflow run")
        click("Cancel Workflow run")
        wait(lambda: "Automatic refresh has stopped" in text() and "Cancelled" in text(), "Cancellation did not settle")
        STATE.update(cancelled=False, polls=0)
        open_page(f"/workflow-runs/{INVOCATION}")
        wait(lambda: "Execution progress" in text(), "Active invocation did not reopen")
        open_page("/workflows")
        time.sleep(.5)
        polls = STATE["polls"]
        time.sleep(5.5)
        assert STATE["polls"] == polls, "Polling continued after leaving route"
        choose("#workflows-namespace-filter", "operations")
        wait(lambda: "postgres-upgrade" in text(), "Catalog did not load again")
        click("Delete")
        wait(lambda: js("return !!document.querySelector('dialog[open]')"), "Delete confirmation absent")
        click("Confirm delete")
        wait(lambda: "Workflow with execution history cannot be deleted" in text(), "Delete guard error hidden")
        click("Cancel")
        STATE["history"] = False
        click("Delete")
        click("Confirm delete")
        wait(lambda: "No workflows yet" in text(), "Successful delete did not refresh list")
        late_launch_check()
        reload_failure_check()
        form_controls_check()
        structured_inputs_check()
        print("Workflow browser checks passed: CRUD, namespace isolation, editor errors/removal/revision recovery, Target Set launch retry, immutable progress, skips/Run links, polling/cleanup, cancellation confirmation, delete guard, responsive layout")
    finally:
        if session:
            request(base, "DELETE")
        driver.terminate()
        driver.wait(timeout=10)
        server.shutdown()
        server.server_close()

if __name__ == "__main__":
    main()
