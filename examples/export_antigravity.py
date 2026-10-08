#!/usr/bin/env python3
"""macOS: export a full native trajectory through the running Antigravity app.

Usage: python3 examples/export_antigravity.py CONVERSATION_ID OUTPUT.trajectory.json
No model request or authentication change is performed. The exporter does not
write native transcript files. The app may initialize its storage and load a
persisted trajectory into memory. Existing outputs are not
replaced. Exported files are private (0600); request headers are excluded.
"""
import json
import os
from pathlib import Path
import re
import ssl
import subprocess
import sys
import urllib.error
import urllib.request


def export(conversation_id):
    process_list = subprocess.check_output(["ps", "-axo", "pid,command"], text=True)
    for line in process_list.splitlines():
        if "/Applications/Antigravity.app/Contents/Resources/bin/language_server " not in line:
            continue
        token = re.search(r"--csrf_token[ =](\S+)", line)
        if not token:
            continue
        pid = line.strip().split()[0]
        listeners = subprocess.run(
            ["lsof", "-nP", "-a", "-p", pid, "-iTCP", "-sTCP:LISTEN"],
            capture_output=True, text=True, check=False,
        ).stdout
        for port in re.findall(r"127\.0\.0\.1:(\d+)", listeners):
            for scheme in ("https", "http"):
                address = f"{scheme}://127.0.0.1:{port}"

                def rpc(method, payload):
                    request = urllib.request.Request(
                        address + "/exa.language_server_pb.LanguageServerService/" + method,
                        data=json.dumps(payload).encode(),
                        headers={"content-type": "application/json",
                                 "x-codeium-csrf-token": token.group(1)},
                    )
                    # The vendor uses a self-signed certificate on its loopback
                    # service. No remote address is accepted by this exporter.
                    with urllib.request.urlopen(
                        request, context=ssl._create_unverified_context(), timeout=30,
                    ) as response:
                        return json.load(response)

                try:
                    rpc("GetAllCascadeTrajectories", {})
                except (urllib.error.URLError, json.JSONDecodeError):
                    continue
                rpc("LoadTrajectory", {"cascadeId": conversation_id})
                result = rpc("GetCascadeTrajectory", {
                    "cascadeId": conversation_id,
                    "trajectoryVerbosity": "CLIENT_TRAJECTORY_VERBOSITY_FULL",
                })
                trajectory = result["trajectory"]
                steps = trajectory["steps"]
                if int(result["numTotalSteps"]) != len(steps):
                    raise ValueError("native service returned a truncated trajectory")
                # Retain only fields consumed by the importer. Service metadata
                # contains internal headers and is not a transcript event.
                for step in steps:
                    metadata = step.get("metadata", {})
                    step["metadata"] = {
                        key: value for key, value in metadata.items()
                        if key in ("createdAt", "toolCall", "modelUsage")
                    }
                    step["metadata"].get("modelUsage", {}).pop("responseHeader", None)
                    step.get("userInput", {}).pop("userConfig", None)
                    step.get("userInput", {}).pop("activeUserState", None)
                return {"trajectory": {"cascadeId": trajectory.get("cascadeId", conversation_id),
                                       "steps": steps}, "numTotalSteps": len(steps)}
    raise RuntimeError("no reachable Antigravity language service; start the native app")


def main():
    if len(sys.argv) != 3:
        raise ValueError("usage: export_antigravity.py CONVERSATION_ID OUTPUT.trajectory.json")
    result = export(sys.argv[1])
    output = Path(sys.argv[2])
    data = json.dumps(result, ensure_ascii=False).encode()
    with os.fdopen(os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb") as file:
        file.write(data)
    print(json.dumps({"steps": result["numTotalSteps"], "exported": True}))


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, ValueError, KeyError, OSError) as error:
        # URL exceptions may contain internal diagnostics. Never print tokens,
        # service response bodies or credentials.
        print("export failed: " + type(error).__name__, file=sys.stderr)
        sys.exit(1)
