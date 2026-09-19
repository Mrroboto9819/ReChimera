import os
import sys
import json
import traceback

OUT_DIR = r"C:\Users\flast\AppData\Local\Temp\claude\C--Users-flast-Documents-project-ReChimera\93ec6b94-f8d0-40f9-948a-8679999b3bdf\scratchpad\renderdoc_export_live"
CAPTURE = r"C:\Users\flast\AppData\Local\Temp\RenderDoc\rpcs3_2026.09.19_09.08.45_frame3338.rdc"

os.makedirs(OUT_DIR, exist_ok=True)
log = open(os.path.join(OUT_DIR, "recon_log.txt"), "w", buffering=1)

def w(msg):
    log.write(msg + "\n")

try:
    import renderdoc as rd
    w("renderdoc module loaded, version: " + rd.GetVersionString())

    def ok(r):
        try:
            return r.OK()
        except AttributeError:
            return r == rd.ReplayStatus.Succeeded

    cap = rd.OpenCaptureFile()
    res = cap.OpenFile(CAPTURE, '', None)
    if not ok(res):
        w("FAILED to open file: " + str(res))
        os._exit(1)
    w("capture file opened")

    if not cap.LocalReplaySupport():
        w("FAILED: no local replay support")
        os._exit(1)

    res, controller = cap.OpenCapture(rd.ReplayOptions(), None)
    if not ok(res):
        w("FAILED to open replay: " + str(res))
        os._exit(1)
    w("replay opened")

    sf = controller.GetStructuredFile()

    draws = []

    def visit(actions, depth):
        for a in actions:
            entry = {
                "eventId": a.eventId,
                "depth": depth,
                "flags": int(a.flags),
            }
            try:
                entry["name"] = a.GetName(sf)
            except Exception:
                entry["name"] = ""
            try:
                entry["numIndices"] = a.numIndices
                entry["numInstances"] = a.numInstances
            except Exception:
                pass
            entry["isDraw"] = bool(a.flags & rd.ActionFlags.Drawcall)
            draws.append(entry)
            visit(a.children, depth + 1)

    visit(controller.GetRootActions(), 0)
    w("actions flattened: %d total, %d draws" % (len(draws), sum(1 for d in draws if d["isDraw"])))

    textures = []
    tex_list = controller.GetTextures()
    w("textures: %d" % len(tex_list))
    for t in tex_list:
        entry = {
            "resourceId": str(t.resourceId),
            "width": t.width,
            "height": t.height,
            "depth": t.depth,
            "arraysize": t.arraysize,
            "mips": t.mips,
            "format": t.format.Name(),
            "creationFlags": int(t.creationFlags),
        }
        try:
            usage = controller.GetUsage(t.resourceId)
            entry["usage"] = [{"eventId": u.eventId, "usage": str(u.usage)} for u in usage]
        except Exception as e:
            entry["usage"] = []
            entry["usageError"] = str(e)
        textures.append(entry)
    w("texture usage collected")

    with open(os.path.join(OUT_DIR, "draws.json"), "w") as f:
        json.dump(draws, f, indent=1)
    with open(os.path.join(OUT_DIR, "textures.json"), "w") as f:
        json.dump(textures, f, indent=1)

    w("DONE - manifests written")
    controller.Shutdown()
    cap.Shutdown()
except Exception:
    w("EXCEPTION:\n" + traceback.format_exc())
finally:
    log.close()
    os._exit(0)
