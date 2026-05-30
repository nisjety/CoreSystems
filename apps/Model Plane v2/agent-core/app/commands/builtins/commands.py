"""Built-in slash command implementations.

Each command is a small class implementing the Command protocol.
Grouped in a single file for efficiency — CC has one file per command
but for agent-core we keep related commands together.
"""

from __future__ import annotations

from typing import Any

from app.commands.base import Command, CommandResult, CommandType


# ── Helper base classes ────────────────────────────────────────

class _PromptCmd:
    """Base for prompt-injection commands."""

    command_type = CommandType.PROMPT
    aliases: list[str] = []
    usage: str = ""
    hidden: bool = False

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        return CommandResult(inject_prompt=self._prompt(args, context))

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        return ""


class _LocalCmd:
    """Base for local-execution commands."""

    command_type = CommandType.LOCAL
    aliases: list[str] = []
    usage: str = ""
    hidden: bool = False


# ── Session Management ─────────────────────────────────────────

class CompactCommand(_LocalCmd):
    name = "compact"
    description = "Compact conversation context to free up tokens"
    aliases = ["c"]
    usage = "/compact [--hard]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        hard = "--hard" in args
        return CommandResult(
            output=f"Context compaction {'(hard)' if hard else '(soft)'} requested.",
            metadata={"hard": hard, "action": "compact"},
        )


class ClearCommand(_LocalCmd):
    name = "clear"
    description = "Clear conversation and start fresh"
    aliases = ["reset"]
    usage = "/clear"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        return CommandResult(
            output="Conversation cleared.", metadata={"action": "clear"}
        )


class ExitCommand(_LocalCmd):
    name = "exit"
    description = "End the current session"
    aliases = ["quit", "q"]
    usage = "/exit"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        return CommandResult(
            output="Session ended.", metadata={"action": "exit"}
        )


# ── Code Review / Analysis ────────────────────────────────────

class ReviewCommand(_PromptCmd):
    name = "review"
    description = "Review recent code changes"
    aliases = ["cr"]
    usage = "/review [file_path]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        target = args.strip() or "the recent changes"
        return f"Please review {target} for bugs, security issues, and code quality. Provide specific, actionable feedback."


class DiffCommand(_PromptCmd):
    name = "diff"
    description = "Show and analyze git diff"
    usage = "/diff [branch]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        branch = args.strip() or "HEAD"
        return f"Show the git diff against {branch}. Summarize the changes and highlight anything concerning."


class DoctorCommand(_PromptCmd):
    name = "doctor"
    description = "Diagnose project health issues"
    usage = "/doctor"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        return "Run a health check on the project. Check for: build errors, test failures, missing dependencies, configuration issues, and security vulnerabilities."


class CommitCommand(_PromptCmd):
    name = "commit"
    description = "Generate a commit message for staged changes"
    usage = "/commit"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        return "Generate a conventional commit message for the currently staged changes. Follow the format: type(scope): description."


class PrCommand(_PromptCmd):
    name = "pr"
    description = "Generate a pull request description"
    usage = "/pr [base_branch]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        base = args.strip() or "main"
        return f"Generate a comprehensive pull request description for changes against {base}. Include: summary, changes, testing, and checklist."


# ── Planning & Organization ───────────────────────────────────

class PlanCommand(_PromptCmd):
    name = "plan"
    description = "Enter plan mode to break down a task"
    usage = "/plan [task_description]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        task = args.strip() or "the current task"
        return f"Create a detailed implementation plan for: {task}. Break it into phases, identify dependencies, and estimate complexity."


class ExitPlanCommand(_LocalCmd):
    name = "exit-plan"
    description = "Exit plan mode and begin execution"
    aliases = ["ep"]
    usage = "/exit-plan"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        return CommandResult(
            output="Plan mode exited. Beginning execution.",
            metadata={"action": "exit_plan"},
        )


class TodoCommand(_LocalCmd):
    name = "todo"
    description = "Show or manage task list"
    usage = "/todo [add|done|list] [description]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        parts = args.strip().split(maxsplit=1)
        action = parts[0] if parts else "list"
        return CommandResult(
            output=f"Todo action: {action}",
            metadata={"action": "todo", "sub_action": action},
        )


# ── Information ────────────────────────────────────────────────

class HelpCommand(_LocalCmd):
    name = "help"
    description = "Show available commands"
    aliases = ["h", "?"]
    usage = "/help [command]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        registry = context.get("registry")
        if args.strip() and registry:
            cmd = registry.get(args.strip())
            if cmd:
                return CommandResult(
                    output=f"/{cmd.name} — {cmd.description}\nUsage: {cmd.usage}"
                )
            return CommandResult(error=f"Unknown command: /{args.strip()}")
        if registry:
            cmds = registry.list_all()
            lines = [f"  /{c.name:20s} {c.description}" for c in cmds]
            return CommandResult(output="Available commands:\n" + "\n".join(lines))
        return CommandResult(output="Help system (no registry in context)")


class CostCommand(_LocalCmd):
    name = "cost"
    description = "Show token usage and cost for this session"
    usage = "/cost"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        tracker = context.get("cost_tracker")
        if tracker:
            s = tracker.summary()
            return CommandResult(
                output=f"Tokens: {s['total_tokens']:,} | USD: ${s['total_usd']:.4f} | Budget: {s['utilization']:.0%}"
            )
        return CommandResult(output="No cost tracker available.")


class StatsCommand(_LocalCmd):
    name = "stats"
    description = "Show session statistics"
    usage = "/stats"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        return CommandResult(
            output="Session stats requested.",
            metadata={"action": "stats"},
        )


class ConfigCommand(_LocalCmd):
    name = "config"
    description = "View or modify configuration"
    usage = "/config [key] [value]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        parts = args.strip().split(maxsplit=1)
        if not parts or not parts[0]:
            return CommandResult(output="Current configuration summary.", metadata={"action": "config_list"})
        key = parts[0]
        value = parts[1] if len(parts) > 1 else None
        if value:
            return CommandResult(output=f"Set {key}={value}", metadata={"action": "config_set", "key": key, "value": value})
        return CommandResult(output=f"Config: {key}", metadata={"action": "config_get", "key": key})


# ── Memory & Context ──────────────────────────────────────────

class MemoryCommand(_PromptCmd):
    name = "memory"
    description = "View or manage agent memory files"
    aliases = ["mem"]
    usage = "/memory [view|edit|delete] [path]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        return f"Manage memory files. Action: {args.strip() or 'view all'}"


class ExportCommand(_LocalCmd):
    name = "export"
    description = "Export conversation as markdown"
    usage = "/export [path]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        path = args.strip() or "conversation.md"
        return CommandResult(
            output=f"Export to {path} requested.",
            metadata={"action": "export", "path": path},
        )


class ContextCommand(_LocalCmd):
    name = "context"
    description = "Show context window utilization"
    aliases = ["ctx"]
    usage = "/context"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        return CommandResult(
            output="Context window status requested.",
            metadata={"action": "context"},
        )


# ── Development Tools ─────────────────────────────────────────

class TestCommand(_PromptCmd):
    name = "test"
    description = "Run or generate tests"
    aliases = ["t"]
    usage = "/test [file_path]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        target = args.strip() or "the current module"
        return f"Run existing tests for {target}. If tests don't exist, generate comprehensive test cases first."


class FixCommand(_PromptCmd):
    name = "fix"
    description = "Fix errors or issues"
    aliases = ["f"]
    usage = "/fix [error_description]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        return f"Fix the following issue: {args.strip() or 'the last error'}"


class RefactorCommand(_PromptCmd):
    name = "refactor"
    description = "Refactor code for better quality"
    usage = "/refactor [file_path]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        target = args.strip() or "the current file"
        return f"Refactor {target} to improve readability, maintainability, and performance."


class ExplainCommand(_PromptCmd):
    name = "explain"
    description = "Explain code or concept"
    usage = "/explain [file_path|concept]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        target = args.strip() or "the selected code"
        return f"Explain {target} in detail. Include purpose, how it works, and any important patterns."


class SearchCommand(_PromptCmd):
    name = "search"
    description = "Search codebase for patterns"
    usage = "/search <query>"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        return f"Search the codebase for: {args.strip()}"


class DebugCommand(_PromptCmd):
    name = "debug"
    description = "Debug an issue with analysis"
    usage = "/debug [description]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        return f"Debug: {args.strip() or 'the current issue'}. Analyze logs, errors, and code flow."


# ── Git / VCS ─────────────────────────────────────────────────

class GitCommand(_PromptCmd):
    name = "git"
    description = "Execute git operations"
    usage = "/git <subcommand>"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        return f"Execute git operation: {args.strip()}"


class StashCommand(_PromptCmd):
    name = "stash"
    description = "Stash or restore changes"
    usage = "/stash [pop|list]"

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        action = args.strip() or "save"
        return f"Git stash {action}"


# ── Agent Control ─────────────────────────────────────────────

class ModelCommand(_LocalCmd):
    name = "model"
    description = "Show or switch the active model"
    usage = "/model [model_name]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        model = args.strip()
        if model:
            return CommandResult(
                output=f"Model switch to {model} requested.",
                metadata={"action": "model_switch", "model": model},
            )
        return CommandResult(
            output="Current model info requested.",
            metadata={"action": "model_info"},
        )


class PermissionsCommand(_LocalCmd):
    name = "permissions"
    description = "View or toggle auto-accept permissions"
    aliases = ["perms"]
    usage = "/permissions [allow|deny|reset]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        action = args.strip() or "show"
        return CommandResult(
            output=f"Permissions: {action}",
            metadata={"action": "permissions", "sub_action": action},
        )


class ThinkCommand(_PromptCmd):
    name = "think"
    description = "Enable extended thinking for next response"
    usage = "/think [budget_tokens]"
    hidden = True

    def _prompt(self, args: str, context: dict[str, Any]) -> str:
        budget = args.strip() or "32000"
        return f"[Extended thinking enabled with budget {budget} tokens]"


class VerboseCommand(_LocalCmd):
    name = "verbose"
    description = "Toggle verbose output mode"
    aliases = ["v"]
    usage = "/verbose [on|off]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        toggle = args.strip().lower()
        if toggle in ("on", "true", "1"):
            return CommandResult(output="Verbose mode ON.", metadata={"verbose": True})
        if toggle in ("off", "false", "0"):
            return CommandResult(output="Verbose mode OFF.", metadata={"verbose": False})
        return CommandResult(output="Verbose mode toggled.", metadata={"verbose": "toggle"})


class UndoCommand(_LocalCmd):
    name = "undo"
    description = "Undo the last file edit"
    aliases = ["u"]
    usage = "/undo"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        return CommandResult(
            output="Undo last edit requested.",
            metadata={"action": "undo"},
        )


# ── New commands: skills, tasks, mcp, resume, share ───────────

class SkillsCommand(_LocalCmd):
    name = "skills"
    description = "List, install, or uninstall agent skills"
    usage = "/skills [list|install <name>|uninstall <name>]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        parts = args.strip().split(maxsplit=1)
        sub = parts[0].lower() if parts else "list"
        arg = parts[1].strip() if len(parts) > 1 else ""

        skills_registry = context.get("skills_registry")
        if sub == "list":
            if skills_registry:
                skills = skills_registry.list_all()
                lines = [f"  {s.name:24s} {s.description}" for s in skills]
                return CommandResult(
                    output="Installed skills:\n" + ("\n".join(lines) or "  (none)"),
                    metadata={"action": "skills_list"},
                )
            return CommandResult(
                output="Skills registry not available in this context.",
                metadata={"action": "skills_list"},
            )
        elif sub == "install" and arg:
            return CommandResult(
                output=f"Install skill '{arg}' requested.",
                metadata={"action": "skills_install", "skill": arg},
            )
        elif sub == "uninstall" and arg:
            return CommandResult(
                output=f"Uninstall skill '{arg}' requested.",
                metadata={"action": "skills_uninstall", "skill": arg},
            )
        return CommandResult(
            error=f"Unknown skills sub-command: '{sub}'. Use: list | install <name> | uninstall <name>"
        )


class TasksCommand(_LocalCmd):
    name = "tasks"
    description = "List, create, or mark tasks done in the current run"
    usage = "/tasks [list|create <subject>|done <task_id>]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        parts = args.strip().split(maxsplit=1)
        sub = parts[0].lower() if parts else "list"
        arg = parts[1].strip() if len(parts) > 1 else ""

        run_id = context.get("run_id", "unknown")
        session_id = context.get("session_id", "unknown")

        if sub == "list":
            try:
                from app.tasks import repository as task_repo
                tasks = await task_repo.list_tasks_for_run(run_id)
                if not tasks:
                    return CommandResult(output="No tasks for current run.", metadata={"action": "tasks_list"})
                lines = [f"  [{t.status.value:11s}] {t.id[:8]}  {t.subject}" for t in tasks]
                return CommandResult(
                    output="Tasks:\n" + "\n".join(lines),
                    metadata={"action": "tasks_list", "count": len(tasks)},
                )
            except Exception as exc:
                return CommandResult(error=f"Failed to list tasks: {exc}")

        elif sub == "create" and arg:
            try:
                from app.tasks.domain import TaskRecord
                from app.tasks import repository as task_repo
                task = TaskRecord(run_id=run_id, session_id=session_id, subject=arg)
                created = await task_repo.create_task(task)
                return CommandResult(
                    output=f"Task created: {created.id}  '{created.subject}'",
                    metadata={"action": "tasks_create", "task_id": created.id},
                )
            except Exception as exc:
                return CommandResult(error=f"Failed to create task: {exc}")

        elif sub == "done" and arg:
            try:
                from app.tasks.domain import TaskStatus, TaskUpdate
                from app.tasks import repository as task_repo
                updated = await task_repo.update_task(arg, TaskUpdate(status=TaskStatus.COMPLETED))
                if updated is None:
                    return CommandResult(error=f"Task not found: {arg}")
                return CommandResult(
                    output=f"Task {arg} marked completed.",
                    metadata={"action": "tasks_done", "task_id": arg},
                )
            except Exception as exc:
                return CommandResult(error=f"Failed to complete task: {exc}")

        return CommandResult(
            error=f"Unknown tasks sub-command: '{sub}'. Use: list | create <subject> | done <task_id>"
        )


class McpCommand(_LocalCmd):
    name = "mcp"
    description = "List, add, remove, or restart MCP server connections"
    usage = "/mcp [list|add <name> <command>|remove <name>|restart <name>]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        parts = args.strip().split(maxsplit=2)
        sub = parts[0].lower() if parts else "list"

        mcp_manager = context.get("mcp_manager")

        if sub == "list":
            if mcp_manager:
                servers = mcp_manager.list_servers()
                if not servers:
                    return CommandResult(output="No MCP servers configured.", metadata={"action": "mcp_list"})
                lines = [f"  {s.name:24s} {s.state}" for s in servers]
                return CommandResult(
                    output="MCP servers:\n" + "\n".join(lines),
                    metadata={"action": "mcp_list", "count": len(servers)},
                )
            return CommandResult(output="MCP manager not available.", metadata={"action": "mcp_list"})

        elif sub == "restart" and len(parts) >= 2:
            name = parts[1]
            if mcp_manager:
                try:
                    await mcp_manager.restart(name)
                    return CommandResult(
                        output=f"MCP server '{name}' restarted.",
                        metadata={"action": "mcp_restart", "server": name},
                    )
                except Exception as exc:
                    return CommandResult(error=f"Failed to restart '{name}': {exc}")
            return CommandResult(error="MCP manager not available.")

        elif sub == "remove" and len(parts) >= 2:
            name = parts[1]
            if mcp_manager:
                try:
                    await mcp_manager.remove(name)
                    return CommandResult(
                        output=f"MCP server '{name}' removed.",
                        metadata={"action": "mcp_remove", "server": name},
                    )
                except Exception as exc:
                    return CommandResult(error=f"Failed to remove '{name}': {exc}")
            return CommandResult(error="MCP manager not available.")

        return CommandResult(
            error=f"Unknown mcp sub-command: '{sub}'. Use: list | restart <name> | remove <name>"
        )


class ResumeCommand(_LocalCmd):
    name = "resume"
    description = "Resume a previously interrupted run by run ID"
    usage = "/resume <run_id>"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        run_id = args.strip()
        if not run_id:
            return CommandResult(error="Usage: /resume <run_id>")
        return CommandResult(
            output=f"Resume run '{run_id}' requested.",
            metadata={"action": "resume", "run_id": run_id},
        )


class ShareCommand(_LocalCmd):
    name = "share"
    description = "Share a summary of the current session or a specific run"
    usage = "/share [run_id]"

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        run_id = args.strip() or context.get("run_id", "current")
        session_id = context.get("session_id", "unknown")
        return CommandResult(
            output=f"Session share link for run '{run_id}' (session: {session_id}) — link generation deferred to host.",
            metadata={"action": "share", "run_id": run_id, "session_id": session_id},
        )


# ── Collection ─────────────────────────────────────────────────

ALL_COMMANDS: list[type] = [
    CompactCommand,
    ClearCommand,
    ExitCommand,
    ReviewCommand,
    DiffCommand,
    DoctorCommand,
    CommitCommand,
    PrCommand,
    PlanCommand,
    ExitPlanCommand,
    TodoCommand,
    HelpCommand,
    CostCommand,
    StatsCommand,
    ConfigCommand,
    MemoryCommand,
    ExportCommand,
    ContextCommand,
    TestCommand,
    FixCommand,
    RefactorCommand,
    ExplainCommand,
    SearchCommand,
    DebugCommand,
    GitCommand,
    StashCommand,
    ModelCommand,
    PermissionsCommand,
    ThinkCommand,
    VerboseCommand,
    UndoCommand,
    # New commands
    SkillsCommand,
    TasksCommand,
    McpCommand,
    ResumeCommand,
    ShareCommand,
]


def register_all_commands(registry: Any) -> None:
    """Register all built-in commands into a CommandRegistry."""
    for cmd_cls in ALL_COMMANDS:
        registry.register(cmd_cls())
