import os
import json
import struct
import traceback

OUT_DIR = r"C:\Users\flast\AppData\Local\Temp\claude\C--Users-flast-Documents-project-ReChimera\93ec6b94-f8d0-40f9-948a-8679999b3bdf\scratchpad\renderdoc_export_cap2"
CAPTURE = r"C:\Users\flast\AppData\Local\Temp\RenderDoc\rpcs3_2026.09.19_10.11.27_frame5167.rdc"
JSONL = os.path.join(OUT_DIR, "postvs_bboxes.jsonl")
ATTEMPT = os.path.join(OUT_DIR, "bbox_attempt.txt")
POISON = os.path.join(OUT_DIR, "bbox_poison.txt")

log = open(os.path.join(OUT_DIR, "bbox_log.txt"), "a", buffering=1)

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

    draws = []

    def visit(acts):
        for a in acts:
            if a.flags & rd.ActionFlags.Drawcall:
                draws.append(a.eventId)
            visit(a.children)

    visit(controller.GetRootActions())
    w("draws: %d" % len(draws))

    out_f = open(JSONL, "a", buffering=1)
    processed = 0
    for eid in draws:
        if eid in done or eid in poison:
            continue
        with open(ATTEMPT, "w") as f:
            f.write(str(eid))
        rec = {"eventId": eid}
        try:
            controller.SetFrameEvent(eid, True)
            mf = controller.GetPostVSData(0, 0, rd.MeshDataStage.VSOut)
            if mf.vertexResourceId != rd.ResourceId.Null() and mf.numIndices > 0:
                n = mf.numIndices
                if mf.indexResourceId != rd.ResourceId.Null() and mf.indexByteStride > 0:
                    idata = controller.GetBufferData(
                        mf.indexResourceId, mf.indexByteOffset, n * mf.indexByteStride)
                    ch = {2: "H", 4: "I"}.get(mf.indexByteStride, "I")
                    idx = struct.unpack("<%d%s" % (n, ch), idata[: n * mf.indexByteStride])
                    uniq = sorted(set(idx))
                else:
                    uniq = list(range(n))
                lo, hi = uniq[0], uniq[-1]
                vdata = controller.GetBufferData(
                    mf.vertexResourceId,
                    mf.vertexByteOffset + lo * mf.vertexByteStride,
                    (hi - lo + 1) * mf.vertexByteStride)
                cw = mf.format.compByteWidth
                pts = 0
                mins = [1e9, 1e9, 1e9]
                maxs = [-1e9, -1e9, -1e9]
                cent = [0.0, 0.0, 0.0]
                if cw == 4:
                    for vi in uniq:
                        off = (vi - lo) * mf.vertexByteStride
                        if off + 16 > len(vdata):
                            continue
                        x, y, z, ww = struct.unpack_from("<4f", vdata, off)
                        if abs(ww) < 1e-9:
                            continue
                        p = (x / ww, y / ww, z / ww)
                        for c in range(3):
                            if p[c] < mins[c]:
                                mins[c] = p[c]
                            if p[c] > maxs[c]:
                                maxs[c] = p[c]
                            cent[c] += p[c]
                        pts += 1
                if pts > 0:
                    rec["count"] = pts
                    rec["min"] = mins
                    rec["max"] = maxs
                    rec["centroid"] = [c / pts for c in cent]
        except Exception as e:
            rec["error"] = str(e)
        out_f.write(json.dumps(rec) + "\n")
        processed += 1
        if processed % 25 == 0:
            w("progress: %d" % processed)

    out_f.close()
    with open(ATTEMPT, "w") as f:
        f.write("")
    w("RUN COMPLETE, processed %d" % processed)
    controller.Shutdown()
    cap.Shutdown()
except Exception:
    w("EXCEPTION:\n" + traceback.format_exc())
finally:
    log.close()
    os._exit(0)
