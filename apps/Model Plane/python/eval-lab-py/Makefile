# Eval harness entry points (docs/EVAL_HARNESS_MVP.md)
#
#   make selftest  — CI path: schema + metric-runner tests, no network
#   make eval      — live path: drives the compose fleet via model-gateway,
#                    writes docs/eval-reports/<date>.md
#                    (requires INTERNAL_API_KEY; auth-core + model-gateway up)

PYTHON ?= python3
VENVDIR ?= .venv

.PHONY: venv selftest eval

venv:
	$(PYTHON) -m venv $(VENVDIR)
	$(VENVDIR)/bin/pip install -q -e ".[dev]"

selftest:
	$(VENVDIR)/bin/pytest tests/test_runner.py -q

eval:
	EVAL_LIVE=1 $(VENVDIR)/bin/pytest tests/test_live_suite.py -q -rs
