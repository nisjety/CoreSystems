---
id: submit-feedback
title: Reporting a problem with this server
description: When and how to file a feedback report with the operator about a wrong or missing skill, a misleading result or a bug, what to include, and what never to include.
tags: [feedback, report, skills, quality]
tools: [visma_submit_feedback]
primary: [visma_submit_feedback]
prerequisites: [start-here]
difficulty: beginner
steps: 3
---
# Reporting a problem with this server

`visma_submit_feedback` stores a short report for the operator in a local file. Nothing is sent to Visma or to
anyone else. File one when a skill was wrong or missing, when a result looked wrong, when a tool failed in a way the
operator should know about, or when the user asks for it.

## 1. Say what happened

Use the `kind` that fits: wrong_skill, missing_skill, wrong_result, bug or suggestion. Name the tool and the skill
involved when there is one. Write the message in plain words: what was done, what was expected, what happened.

```tool-call
visma_submit_feedback
{"kind": "suggestion", "message": "Example report from the skill check. Nothing is sent to Visma.", "skill": "submit-feedback"}
```

## 2. Never include secrets

Do not put credentials, tokens, passwords, client secrets, personal data or customer details in the message.
Describe the problem with ids and field names instead.

## 3. Tell the user

Say that the report was stored, and where, so the user knows it stayed on this machine. The operator decides what
to do with it.
