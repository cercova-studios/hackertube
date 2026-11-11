# ADR 0001: Agent-first CLI, no product UI

## Status

Accepted

## Decision

HackerTube is a console script. Commands are small verbs (`recommend`,
`search`, `insights`, `seen`). There is no web viewer, server, or account.

## Why this is better for agents

An agent already has a shell. A page of HTML costs tokens and hides state.
A predictable command plus a document on stdout can be grepped, retried, and
composed. The home view with no arguments is the live catalog, not a usage
banner, so the first call returns data.

## Consequences

Rendering stays in `output.py`. A UI would be a different project.
