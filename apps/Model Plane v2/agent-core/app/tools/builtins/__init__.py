"""Built-in tools — the core tools that ship with agent-core.

Core tools (10):  bash, file_read, file_edit, file_write, grep, glob,
                  web_search, web_fetch, agent, tool_search
Subsystem wrappers (9): task_create, task_update, send_message,
                        team_create, team_delete, enter_plan_mode,
                        exit_plan_mode, cron_create, lsp
Utility tools (3): sleep, remote_trigger, synthetic_output
"""

from __future__ import annotations

from app.tools.builtins.bash import BashTool
from app.tools.builtins.file_read import FileReadTool
from app.tools.builtins.file_edit import FileEditTool
from app.tools.builtins.file_write import FileWriteTool
from app.tools.builtins.grep import GrepTool
from app.tools.builtins.glob import GlobTool
from app.tools.builtins.web_search import WebSearchTool
from app.tools.builtins.web_fetch import WebFetchTool
from app.tools.builtins.extract_structured import ExtractStructuredTool
from app.tools.builtins.agent import AgentTool
from app.tools.builtins.tool_search import ToolSearchTool

# Subsystem wrappers
from app.tools.builtins.task_create import TaskCreateTool, TaskUpdateTool
from app.tools.builtins.send_message import SendMessageTool
from app.tools.builtins.team_create import TeamCreateTool, TeamDeleteTool
from app.tools.builtins.enter_plan_mode import EnterPlanModeTool, ExitPlanModeTool
from app.tools.builtins.cron_create import CronCreateTool
from app.tools.builtins.lsp_tool import LSPTool

# Knowledge tools (Data Plane integration)
from app.tools.builtins.knowledge_search import KnowledgeSearchTool

# Utility tools
from app.tools.builtins.sleep import SleepTool
from app.tools.builtins.remote_trigger import RemoteTriggerTool
from app.tools.builtins.synthetic_output import SyntheticOutputTool

from app.tools.registry import ToolRegistry

ALL_BUILTINS = [
    # Core
    BashTool,
    FileReadTool,
    FileEditTool,
    FileWriteTool,
    GrepTool,
    GlobTool,
    WebSearchTool,
    WebFetchTool,
    ExtractStructuredTool,
    AgentTool,
    ToolSearchTool,
    # Subsystem wrappers
    TaskCreateTool,
    TaskUpdateTool,
    SendMessageTool,
    TeamCreateTool,
    TeamDeleteTool,
    EnterPlanModeTool,
    ExitPlanModeTool,
    CronCreateTool,
    LSPTool,
    # Knowledge (Data Plane)
    KnowledgeSearchTool,
    # Utility
    SleepTool,
    RemoteTriggerTool,
    SyntheticOutputTool,
]


def register_builtins(registry: ToolRegistry) -> None:
    """Instantiate and register all built-in tools."""
    for tool_cls in ALL_BUILTINS:
        registry.register(tool_cls())
