"""Trajectory package — records agent run traces for self-improvement.

Public surface:
    from app.trajectory import TrajectoryRecorder, normalize_goal
"""

from app.trajectory.patterns import normalize_goal
from app.trajectory.recorder import TrajectoryRecorder

__all__ = ["TrajectoryRecorder", "normalize_goal"]
