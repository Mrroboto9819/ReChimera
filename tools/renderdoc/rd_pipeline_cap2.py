import os
import json
import traceback

OUT_DIR = r"C:\Users\flast\AppData\Local\Temp\claude\C--Users-flast-Documents-project-ReChimera\93ec6b94-f8d0-40f9-948a-8679999b3bdf\scratchpad\renderdoc_export_cap2"
CAPTURE = r"C:\Users\flast\AppData\Local\Temp\RenderDoc\rpcs3_2026.09.19_10.11.27_frame5167.rdc"
TEX_DIR = os.path.join(OUT_DIR, "textures")
JSONL = os.path.join(OUT_DIR, "draw_textures.jsonl")
ATTEMPT = os.path.join(OUT_DIR, "attempt_marker.txt")
POISON = os.path.join(OUT_DIR, "poison_eids.txt")

os.makedirs(TEX_DIR, exist_ok=True)
log = open(os.path.join(OUT_DIR, "pipeline_log.txt"), "a", buffering=1)

def w(msg):
    log.write(msg + "\n")

try:
    import renderdoc as rd

    done = set()
    if os.path.exists(JSONL):
        with open(JSONL) as f:
            for line in f:
                try:
                    done.add(json.loads(line)["eventId"])
                except Exception:
                    pass
    poison = set()
    if os.path.exists(POISON):
        with open(POISON) as f:
            poison = {int(x) for x in f.read().split() if x.strip()}
    if os.path.exists(ATTEMPT):
        with open(ATTEMPT) as f:
            txt = f.read().strip()
        if txt:
            stuck = int(txt)
            if stuck not in done:
                poison.add(stuck)
                with open(POISON, "w") as f:
                    f.write(" ".join(str(x) for x in sorted(poison)))
                w("poisoned eid %d" % stuck)

    def ok(r):
        try:
            return r.OK()
        except AttributeError:
            return r == rd.ReplayStatus.Succeeded

    cap = rd.OpenCaptureFile()
    if not ok(cap.OpenFile(CAPTURE, '', None)):
        w("open file FAILED")
        os._exit(1)
    res, controller = cap.OpenCapture(rd.ReplayOptions(), None)
    if not ok(res):
        w("open replay FAILED")
        os._exit(1)
    w("replay opened, done=%d poison=%s" % (len(done), sorted(poison)))

    def rid_int(resid):
        s = str(resid)
        try:
            return int(s.split("::")[-1])
        except ValueError:
            return -1

    def res_of(b):
        for attr in ("resource", "resourceId"):
            if hasattr(b, attr):
                v = getattr(b, attr)
                if isinstance(v, rd.ResourceId):
                    return v
        if hasattr(b, "descriptor"):
            return res_of(b.descriptor)
        return rd.ResourceId.Null()

    if not done and not os.listdir(TEX_DIR):
        saved = 0
        for t in controller.GetTextures():
            n = rid_int(t.resourceId)
            fname = "tex_%05d_%dx%d_%s.png" % (n, t.width, t.height, t.format.Name())
            ts = rd.TextureSave()
            ts.resourceId = t.resourceId
            ts.mip = 0
            ts.slice.sliceIndex = 0
            ts.alpha = rd.AlphaMapping.Discard
            ts.destType = rd.FileType.PNG
            try:
                if controller.SaveTexture(ts, os.path.join(TEX_DIR, fname)):
                    saved += 1
            except Exception:
                pass
        w("textures saved: %d" % saved)

    draws = []

    def visit(acts):
        for a in acts:
            if a.flags & rd.ActionFlags.Drawcall:
                draws.append((a.eventId, a.numIndices))
            visit(a.children)

    visit(controller.GetRootActions())
    w("draws: %d" % len(draws))

    out_f = open(JSONL, "a", buffering=1)
    processed = 0
    for eid, nidx in draws:
        if eid in done or eid in poison:
            continue
        with open(ATTEMPT, "w") as f:
            f.write(str(eid))
        texids = []
        err = ""
        try:
            controller.SetFrameEvent(eid, True)
            pipe = controller.GetPipelineState()
            ro = pipe.GetReadOnlyResources(rd.ShaderStage.Fragment)
            for entry in ro:
                r = res_of(entry)
                n = rid_int(r)
                if n > 0:
                    texids.append(n)
        except Exception as e:
            err = str(e)
        rec = {"eventId": eid, "numIndices": nidx, "textures": sorted(set(texids))}
        if err:
            rec["error"] = err
        out_f.write(json.dumps(rec) + "\n")
        processed += 1
        if processed % 25 == 0:
            w("progress: %d new this run" % processed)

    out_f.close()
    with open(ATTEMPT, "w") as f:
        f.write("")
    w("RUN COMPLETE, processed %d, total %d/%d" % (processed, len(done) + processed, len(draws)))
    controller.Shutdown()
    cap.Shutdown()
except Exception:
    w("EXCEPTION:\n" + traceback.format_exc())
finally:
    log.close()
    os._exit(0)
