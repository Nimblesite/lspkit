---
layout: layouts/blog.njk
title: Build the engine. Share the intelligence.
description: A small architectural idea that keeps language tools working from the same picture.
date: 2026-10-31
author: lspkit
tags: [posts, architecture]
category: field-notes
excerpt: One source of analysis. Two ways to use it. A quieter foundation for developer tools.
---
<!-- [WEB-CONTENT] The single example journal post. -->
![One orange engine routes to two dark terminal surfaces, representing LSP and MCP.](/assets/images/engine.svg)

The editor and the assistant should see the same project. That sounds obvious, until each grows its own caches, background jobs and interpretation of what changed.

## Start with one source of truth

In lspkit, a shared engine owns analysis. The language server and the MCP surface consume it. A change enters once; the resulting knowledge can serve both.

The diagram above is the whole idea: one engine at the centre, with two distinct interfaces connected to the same foundation.

## Make freshness explicit

An analysis result carries a generation. Consumers can tell which state produced it, rather than guessing whether a cached answer is still current. Cancellation and progress belong at this shared boundary too.

> A useful abstraction makes the important thing visible.

## Leave room for the interesting work

Framing messages, tracking open files and coordinating background work are necessary infrastructure. Sharing those pieces leaves more room for the language-specific features people actually use.

That's the starting point. A small foundation, with room to build.

[Explore the toolkit →](/docs/)
