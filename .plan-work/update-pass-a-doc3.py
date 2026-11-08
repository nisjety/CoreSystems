import os

os.chdir("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567")
p = "docs/plans/space-depth-pass-a-and-ui-parity.md"
b = open(p, "rb").read().decode("utf-8")
crlf = "\r\n" in b
b = b.replace("\r\n", "\n")

start = b.index("| D1-401 `POST /v1/internal/mail-task/prepare-thread`")
end = b.index("\n", start)
new_row = (
    "| D1-401 `POST /v1/internal/mail-task/prepare-thread` | CORRECTION of my earlier reading. Control already refuses: "
    "`IssueMailTaskThreadDecision` rejects an org whose effect policy has `ZeroDataRetention` (tested in `spaces` and `http`); I had only searched the `mailtask` package. "
    "What was missing was Session Core, which did not check a ZDR claim at all. The pipeline behind the route is not wired (`mail_task_pipeline` is `None` in every build). "
    "| FIXED. The signed decision now states `zero_data_retention:false` explicitly (Control also refuses to sign a true one), and Session Core requires the claim and refuses true or missing, with tests on both sides. Row reclassified `refuse`. "
    "Remaining, general to every Control decision family: user-core's effect-policy ZDR bit and org-core's interactive ZDR switch are separate sources and user-core never reads org-core's posture; whether provisioning keeps them in step is unverified. |"
)
b = b[:start] + new_row + b[end:]
if crlf:
    b = b.replace("\n", "\r\n")
open(p, "wb").write(b.encode("utf-8"))
print("doc updated")
