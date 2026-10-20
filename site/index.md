---
layout: home
title: nexc-engine
titleTemplate: Documentation

hero:
  name: nexc-engine
  text: The Enterprise Business Executor. Hand an issue to agents, watch them do the work, prove what they did.
  tagline: A self-hosted work tracker where agents plan and run each issue and report back on it. Every agent is a principal, every action is a span on a hash chain, every policy is versioned data.
  actions:
    - theme: brand
      text: Try it in 60 seconds
      link: /start/install
    - theme: alt
      text: Secure it
      link: /secops/
    - theme: alt
      text: Operate it
      link: /operate/

features:
  - title: Try it
    details: Install with Docker Compose in a minute. Explore the whole product in demo mode without an API key, then plug in Claude, the Claude Code CLI or a local model.
    link: /start/
    linkText: Start
  - title: Trust it
    details: Agent identity, versioned policies that only narrow, a tamper-evident trace, the tools gateway, the Agent Firewall API and A2A on the firewall.
    link: /secops/
    linkText: AgentSecOps
  - title: Operate it
    details: Docker Compose, Kubernetes and Helm, backups and workspace transfers, alert routing, SIEM pulls and the OpenTelemetry export.
    link: /operate/
    linkText: Operate
---

<div class="home-shot">
  <img class="light-only" src="/media/hero-light.png" alt="nexc-engine: a node graph planned by an LLM and executed by agents, light theme">
  <img class="dark-only" src="/media/hero.png" alt="nexc-engine: a node graph planned by an LLM and executed by agents, dark theme">
</div>
