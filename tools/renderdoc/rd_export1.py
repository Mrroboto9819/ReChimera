import os
import sys
import json
import traceback

OUT_DIR = r"C:\Users\flast\AppData\Local\Temp\claude\C--Users-flast-Documents-project-ReChimera\93ec6b94-f8d0-40f9-948a-8679999b3bdf\scratchpad\renderdoc_export_live"
CAPTURE = r"C:\Users\flast\AppData\Local\Temp\RenderDoc\rpcs3_2026.09.19_09.08.45_frame3338.rdc"
TEX_DIR = os.path.join(OUT_DIR, "textures")
RT_DIR = os.path.join(OUT_DIR, "rt_progression")

os.makedirs(TEX_DIR, exist_ok=True)
os.makedirs(RT_DIR, exist_ok=True)
log = open(os.path.join(OUT_DIR, "export1_log.txt"), "w", buffering=1)

def w(msg):
    log.write(msg + "\n")

def rid_int(resid):
    s = str(resid)
    try:
        return int(s.split("::")[-1])
    except ValueError:
        return -1

try:
    import renderdoc as rd

    def ok(r):
        try:
            return r.OK()
        except AttributeError:
            return r == rd.ReplayStatus.Succeeded

    cap = rd.OpenCaptureFile()
    if not ok(cap.OpenFile(CAPTURE, '', None)):
        w("FAILED to open file")
        os._exit(1)
    res, controller = cap.OpenCapture(rd.ReplayOptions(), None)
    if not ok(res):
        w("FAILED to open replay")
        os._exit(1)
    w("replay opened")

    def res_of(b):
        for attr in ("resource", "resourceId"):
            if hasattr(b, attr):
                v = getattr(b, attr)
                if isinstance(v, rd.ResourceId):
                    return v
        if hasattr(b, "descriptor"):
            return res_of(b.descriptor)
        return rd.ResourceId.Null()

    def save_tex(resid, path):
        ts = rd.TextureSave()
        ts.resourceId = resid
        ts.mip = 0
        ts.slice.sliceIndex = 0
        ts.alpha = rd.AlphaMapping.Discard
        ts.destType = rd.FileType.PNG
        return controller.SaveTexture(ts, path)

    tex_list = controller.GetTextures()
    saved = 0
    for t in tex_list:
        n = rid_int(t.resourceId)
        fname = "tex_%05d_%dx%d_%s.png" % (n, t.width, t.height, t.format.Name())
        try:
            if save_tex(t.resourceId, os.path.join(TEX_DIR, fname)):
                saved += 1
        except Exception as e:
            w("tex %d save failed: %s" % (n, str(e)))
    w("textures saved: %d / %d" % (saved, len(tex_list)))

    draws = []

    def visit(actions):
        for a in actions:
            if a.flags & rd.ActionFlags.Drawcall:
                draws.append(a.eventId)
            visit(a.children)

    visit(controller.GetRootActions())
    w("draws: %d" % len(draws))

    rt_manifest = []
    for i, eid in enumerate(draws):
        if i % 5 != 0 and i != len(draws) - 1:
            continue
        controller.SetFrameEvent(eid, True)
        pipe = controller.GetPipelineState()
        try:
            targets = pipe.GetOutputTargets()
        except Exception as e:
            w("eid %d GetOutputTargets failed: %s" % (eid, str(e)))
            continue
        resid = rd.ResourceId.Null()
        for tgt in targets:
            resid = res_of(tgt)
            if resid != rd.ResourceId.Null():
                break
        if resid == rd.ResourceId.Null():
            continue
        fname = "rt_%04d_eid%05d_res%d.png" % (i, eid, rid_int(resid))
        try:
            if save_tex(resid, os.path.join(RT_DIR, fname)):
                rt_manifest.append({"drawIndex": i, "eventId": eid, "rt": rid_int(resid)})
        except Exception as e:
            w("rt at eid %d save failed: %s" % (eid, str(e)))

    with open(os.path.join(OUT_DIR, "rt_manifest.json"), "w") as f:
        json.dump(rt_manifest, f, indent=1)

    w("rt snapshots saved: %d" % len(rt_manifest))
    w("DONE")
    controller.Shutdown()
    cap.Shutdown()
except Exception:
    w("EXCEPTION:\n" + traceback.format_exc())
finally:
    log.close()
    os._exit(0)
