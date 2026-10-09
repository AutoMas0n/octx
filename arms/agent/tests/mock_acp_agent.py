#!/usr/bin/env python3
"""A configurable mock ACP agent for the octx agent-arm integration tests.

Speaks newline-delimited JSON-RPC 2.0 over stdio. Behaviour is switched with
environment variables (see the test file), and every request it receives is
appended to $MOCK_LOG as a JSON line for assertions.
"""
import json
import os
import select
import sys
import time


LOG = os.environ.get("MOCK_LOG")


def log(obj):
    if LOG:
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write(json.dumps(obj) + "\n")


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


class MockAgent:
    def __init__(self):
        self.next_id = 1000
        self.session_id = "sess_mock_1"
        self.cwd = os.getcwd()

    # --- transport -----------------------------------------------------
    def read(self):
        while True:
            line = sys.stdin.readline()
            if not line:
                return None
            line = line.strip()
            if line:
                return json.loads(line)

    def call(self, method, params):
        rid = self.next_id
        self.next_id += 1
        send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
        while True:
            message = self.read()
            if message is None:
                raise SystemExit(1)
            if message.get("id") == rid and (
                "result" in message or "error" in message
            ):
                return message
            self.handle(message)

    def notify(self, method, params):
        send({"jsonrpc": "2.0", "method": method, "params": params})

    def respond(self, rid, result):
        send({"jsonrpc": "2.0", "id": rid, "result": result})

    # --- capabilities --------------------------------------------------
    def capabilities(self):
        caps = {
            "loadSession": False,
            "promptCapabilities": {},
            "mcpCapabilities": {},
            "sessionCapabilities": {},
            "auth": {},
        }
        session_caps = {}
        if os.environ.get("MOCK_RESUME") == "1":
            session_caps["resume"] = {}
        if os.environ.get("MOCK_CLOSE") == "1":
            session_caps["close"] = {}
        caps["sessionCapabilities"] = session_caps
        return caps

    def config_options(self):
        if os.environ.get("MOCK_CONFIG") != "1":
            return None
        return [
            {
                "id": "model",
                "name": "Model",
                "type": "select",
                "currentValue": "default",
                "options": [
                    {"value": "default", "name": "Default"},
                    {"value": "mock-model", "name": "Mock"},
                ],
            },
            {
                "id": "thought_level",
                "name": "Thinking",
                "type": "select",
                "currentValue": "off",
                "options": [
                    {"value": "off", "name": "Off"},
                    {"value": "high", "name": "High"},
                ],
            },
        ]

    # --- dispatch ------------------------------------------------------
    def handle(self, message):
        method = message.get("method")
        rid = message.get("id")
        params = message.get("params", {})
        if method == "initialize":
            caps = self.capabilities()
            response = {
                "protocolVersion": 1,
                "agentCapabilities": caps,
                "authMethods": [],
            }
            if os.environ.get("MOCK_AUTH") == "1":
                response["authMethods"] = [{"id": "mock", "name": "Mock Auth"}]
            elif os.environ.get("MOCK_AUTH_TERMINAL") == "1":
                response["authMethods"] = [
                    {
                        "id": "pi_terminal_login",
                        "name": "Terminal login",
                        "type": "terminal",
                        "args": [],
                        "env": {},
                    }
                ]
            log({
                "method": "initialize",
                "params": params,
                "response": response,
                "env_github": os.environ.get("OCTX_TOKEN_GITHUB"),
            })
            self.respond(rid, response)
        elif method == "authenticate":
            log({"method": "authenticate", "params": params})
            self.respond(rid, {})
        elif method == "session/new":
            self.cwd = params.get("cwd", self.cwd)
            log({"method": "session/new", "params": params})
            result = {"sessionId": self.session_id}
            options = self.config_options()
            if options is not None:
                result["configOptions"] = options
            self.respond(rid, result)
        elif method == "session/resume":
            self.session_id = params.get("sessionId", self.session_id)
            log({"method": "session/resume", "params": params})
            self.respond(rid, {"sessionId": self.session_id})
        elif method == "session/close":
            log({"method": "session/close", "params": params})
            self.respond(rid, {})
        elif method == "session/set_config_option":
            log({"method": "session/set_config_option", "params": params})
            self.respond(rid, {"configOptions": self.config_options() or []})
        elif method == "session/prompt":
            self.prompt(message)
        elif method == "session/cancel":
            log({"method": "session/cancel", "params": params})
        else:
            log({"method": method, "params": params, "unknown": True})
            if rid is not None:
                self.respond(rid, {})

    # --- tools ---------------------------------------------------------
    def run_tools(self):
        sid = self.session_id
        created = self.call(
            "terminal/create",
            {
                "sessionId": sid,
                "command": "bash",
                "args": ["-lc", "echo hello-tool; exit 3"],
                "env": [],
            },
        )
        terminal_id = created.get("result", {}).get("terminalId")
        output = self.call(
            "terminal/wait_for_exit", {"sessionId": sid, "terminalId": terminal_id}
        )
        snapshot = self.call(
            "terminal/output", {"sessionId": sid, "terminalId": terminal_id}
        )
        log(
            {
                "terminal_create": created.get("result"),
                "terminal_output": snapshot.get("result"),
                "terminal_exit": output.get("result"),
            }
        )
        self.call(
            "terminal/release", {"sessionId": sid, "terminalId": terminal_id}
        )

        target = os.path.join(self.cwd, "tool_out.txt")
        self.call(
            "fs/write_text_file",
            {"sessionId": sid, "path": target, "content": "written-by-mock"},
        )
        read = self.call("fs/read_text_file", {"sessionId": sid, "path": target})
        log({"fs_read": read.get("result")})

        escape = os.path.join(self.cwd, "..", "escape.txt")
        escaped = self.call("fs/read_text_file", {"sessionId": sid, "path": escape})
        log({"fs_escape": escaped.get("error")})

        permission = self.call(
            "session/request_permission",
            {
                "sessionId": sid,
                "toolCall": {
                    "toolCallId": "tc_perm",
                    "title": "run thing",
                    "kind": "execute",
                    "status": "pending",
                    "content": [],
                    "locations": [],
                },
                "options": [
                    {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                    {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                ],
            },
        )
        log({"permission": permission.get("result")})

    # --- prompt turn ---------------------------------------------------
    def prompt(self, message):
        rid = message["id"]
        params = message.get("params", {})
        parts = [
            block.get("text", "")
            for block in params.get("prompt", [])
            if block.get("type") == "text"
        ]
        text = "".join(parts)
        log({"method": "session/prompt", "params": params, "text": text})
        self.notify(
            "session/update",
            {
                "sessionId": self.session_id,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "echo: " + text},
                },
            },
        )
        if os.environ.get("MOCK_UNKNOWN") == "1":
            self.notify(
                "session/future_notice",
                {"sessionId": self.session_id, "payload": 123},
            )
        if os.environ.get("MOCK_TOOLS") == "1":
            self.run_tools()
        self.notify(
            "session/update",
            {
                "sessionId": self.session_id,
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "tc_mock",
                    "title": "mock tool",
                    "name": "mock",
                    "kind": "other",
                    "status": "in_progress",
                    "content": [],
                    "locations": [],
                },
            },
        )

        cancelled = self.wait(int(os.environ.get("MOCK_SLEEP_MS", "0")))
        if os.environ.get("MOCK_CRASH") == "1":
            log({"crash": True})
            sys.stderr.write("mock agent crashing\n")
            sys.stderr.flush()
            os._exit(7)
        self.respond(rid, {"stopReason": "cancelled" if cancelled else "end_turn"})

    def wait(self, sleep_ms):
        """Sleep, but keep serving cancel notifications.

        Returns True when a cancel arrived.
        """
        deadline = time.time() + sleep_ms / 1000.0
        while True:
            remaining = deadline - time.time()
            if remaining <= 0:
                return False
            ready, _, _ = select.select([sys.stdin], [], [], min(remaining, 0.05))
            if not ready:
                continue
            line = sys.stdin.readline()
            if not line:
                return False
            line = line.strip()
            if not line:
                continue
            message = json.loads(line)
            if message.get("method") == "session/cancel":
                log({"method": "session/cancel", "params": message.get("params")})
                return True
            self.handle(message)


def main():
    agent = MockAgent()
    while True:
        message = agent.read()
        if message is None:
            return
        agent.handle(message)


if __name__ == "__main__":
    main()
