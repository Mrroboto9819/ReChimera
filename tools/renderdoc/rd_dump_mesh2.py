import os
import json
import struct
import traceback

OUT_DIR = r"C:\Users\flast\AppData\Local\Temp\claude\C--Users-flast-Documents-project-ReChimera\93ec6b94-f8d0-40f9-948a-8679999b3bdf\scratchpad\renderdoc_export_live"
CAPTURE = r"C:\Users\flast\AppData\Local\Temp\RenderDoc\rpcs3_2026.09.19_09.08.45_frame3338.rdc"
TARGETS = [1015, 1019]

log = open(os.path.join(OUT_DIR, "dump_mesh2_log.txt"), "w", buffering=1)

def w(msg):
    log.write(msg + "\n")

try:
    import renderdoc as rd

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
    w("replay opened")

    actions = {}

    def visit(acts):
        for a in acts:
            actions[a.eventId] = a
            visit(a.children)

    visit(controller.GetRootActions())

    for eid in TARGETS:
        a = actions.get(eid)
        if a is None:
            w("eid %d missing" % eid)
            continue
        controller.SetFrameEvent(eid, True)
        try:
            mf = controller.GetPostVSData(0, 0, rd.MeshDataStage.VSOut)
        except Exception as e:
            w("eid %d GetPostVSData failed: %s" % (eid, str(e)))
            continue
        w("eid %d: vtxRes=%s off=%d stride=%d fmt=%s compCount=%d compWidth=%d nIdx=%d idxRes=%s status=%s" % (
            eid, str(mf.vertexResourceId), mf.vertexByteOffset, mf.vertexByteStride,
            mf.format.Name(), mf.format.compCount, mf.format.compByteWidth,
            mf.numIndices, str(mf.indexResourceId), str(getattr(mf, "status", ""))))
        if mf.vertexResourceId == rd.ResourceId.Null():
            w("  no postvs data")
            continue

        n = mf.numIndices
        indices = None
        if mf.indexResourceId != rd.ResourceId.Null() and mf.indexByteStride > 0:
            idata = controller.GetBufferData(
                mf.indexResourceId, mf.indexByteOffset, n * mf.indexByteStride)
            ch = {2: "H", 4: "I"}.get(mf.indexByteStride, "I")
            indices = struct.unpack("<%d%s" % (n, ch), idata[: n * mf.indexByteStride])
            uniq = sorted(set(indices))
        else:
            uniq = list(range(n))

        lo, hi = uniq[0], uniq[-1]
        vdata = controller.GetBufferData(
            mf.vertexResourceId,
            mf.vertexByteOffset + lo * mf.vertexByteStride,
            (hi - lo + 1) * mf.vertexByteStride)

        cw = mf.format.compByteWidth
        cc = min(mf.format.compCount, 4)
        pts = []
        for vi in uniq:
            off = (vi - lo) * mf.vertexByteStride
            comps = []
            good = True
            for c in range(cc):
                o = off + c * cw
                if o + cw > len(vdata):
                    good = False
                    break
                if cw == 4:
                    (fv,) = struct.unpack_from("<f", vdata, o)
                elif cw == 2:
                    (fv,) = struct.unpack_from("<e", vdata, o)
                else:
                    good = False
                    break
                comps.append(float(fv))
            if not good or len(comps) < 3:
                continue
            if cc >= 4 and abs(comps[3]) > 1e-9:
                pts.append([comps[0] / comps[3], comps[1] / comps[3], comps[2] / comps[3]])
            else:
                pts.append(comps[:3])
        w("  ndc positions: %d" % len(pts))
        if pts:
            mins = [min(p[c] for p in pts) for c in range(3)]
            maxs = [max(p[c] for p in pts) for c in range(3)]
            cent = [sum(p[c] for p in pts) / len(pts) for c in range(3)]
            w("  bbox min=%s max=%s centroid=%s" % (
                ["%.5f" % v for v in mins], ["%.5f" % v for v in maxs], ["%.5f" % v for v in cent]))
            with open(os.path.join(OUT_DIR, "postvs_eid%d.obj" % eid), "w") as f:
                for p in pts:
                    f.write("v %f %f %f\n" % (p[0], p[1], p[2]))
            with open(os.path.join(OUT_DIR, "postvs_eid%d.json" % eid), "w") as f:
                json.dump({"eventId": eid, "count": len(pts), "min": mins, "max": maxs, "centroid": cent}, f)

    w("DONE")
    controller.Shutdown()
    cap.Shutdown()
except Exception:
    w("EXCEPTION:\n" + traceback.format_exc())
finally:
    log.close()
    os._exit(0)
