import hashlib
import json
import re
import shutil

ROOT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
GOLDEN = ROOT + "/apps/Model Plane/api/contracts/space-surface.v1.json"
raw = open(GOLDEN, "rb").read()
crlf = b"\r\n" in raw
text = raw.decode("utf-8").replace("\r\n", "\n")
d = json.loads(text)
assert json.dumps(d, indent=2, ensure_ascii=False) + "\n" == text, "golden is not canonical indent=2 JSON"

row = next(r for r in d["rows"] if r["id"] == "mg:POST:/v1/internal/mail-task/prepare-thread")
row["zdr_class"] = "refuse"
row["basis"] = "guard:verify_mail_task_thread_decision"
row["reason"] = (
    "Service-internal bridge that creates an empty user-owned thread (ids only). Control refuses to issue the "
    "decision for a ZDR org, and Session Core requires an explicit zero_data_retention:false claim and refuses "
    "true or a missing one (grpc.rs verify_mail_task_thread_decision). The pipeline itself is not wired: "
    "AppState.mail_task_pipeline is None in every build."
)

out = json.dumps(d, indent=2, ensure_ascii=False) + "\n"
data = out.replace("\n", "\r\n").encode("utf-8") if crlf else out.encode("utf-8")
open(GOLDEN, "wb").write(data)
digest = hashlib.sha256(data).hexdigest()
pin = f"{digest} *space-surface.v1.json\n"
old_pin = open(GOLDEN + ".sha256", "rb").read()
open(GOLDEN + ".sha256", "wb").write((pin.replace("\n", "\r\n") if b"\r\n" in old_pin else pin).encode("utf-8"))
for copy in (
    ROOT + "/apps/Model Plane/rust/services/model-gateway/tests/fixtures/",
    ROOT + "/apps/Frontend Plane/verevonv3/apps/gateway/tests/fixtures/",
):
    shutil.copyfile(GOLDEN, copy + "space-surface.v1.json")
    shutil.copyfile(GOLDEN + ".sha256", copy + "space-surface.v1.json.sha256")
sync = ROOT + "/scripts/tests/zdr-contract-sync.test.sh"
s = open(sync, "rb").read().decode("utf-8")
old_hash = re.search(r"space-surface\.v1\.json\|([0-9a-f]{64})", s).group(1)
assert s.count(old_hash) == 1
open(sync, "wb").write(s.replace(old_hash, digest).encode("utf-8"))
print("digest", digest[:16], "was", old_hash[:16])
