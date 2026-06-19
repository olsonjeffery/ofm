# `omprint`

This is the repository for `omprint`: An orchestration harness, built directly
atop [`oh-my-pi`][0], for agentic code delivery.

Pronounced like _imprint_, with a _'ohm'_ prefix instead of _'im'_.

## Value Proposition

### Guardrails

`omprint` provides a more rigid structure around the [_Ralph Wiggum Loop_][1] (hereafter
referred to as _the loop_), helping agentic coders to spend more time
producing high-quality software, instead of fighting with the agent harness

### Builds upon `oh-my-pi`

`omprint` leverages the high-level of polish provided by [`oh-my-pi`][0] (`omp`)
to mediate all LLM model interactions; Users bring their own provider-
configuration, with `omprint` and `omp` handling the rest

### The `bottega` method

`omprint` is descended from [`vdaubry/bottega`][2], which means it is
_task-driven_. What does this mean? From the [bottega announcement][14]:

> A task is not a prompt. A task is a requirement with acceptance criteria.
>
> The task itself, the requirement, and the technical specification must all
> coexist as enduring artifacts that live alongside the implementation, not
> transient inputs to a single session.

This philosphy colors how `bottega` & `omprint` organize, present and execute
work on behalf of its users.

Additionally:

- Tasks, memory and related documentation live **outside** of code repositories
and worktrees `omprint` is used on
- It's implementation is specification-based; everything starts at
`specs/SPEC.md`; read this to begin understanding _how_ `omprint` works and
what is in-scope
- It is a _web-based_ system, with limited CLI capabilities for onboarding and
agent tools only
- It is _multi-user_ and _persistent_ by design; It is meant for teams
that cooperate to ship software (it is also a pleasant system to run as a
solo programmer; It provides safety, durability, auditability, and more)
  - Provider configuration can be global and/or per-user
  - Generally, `omp` extensons & capabilities are global to the `omprint`
  install and shared by all users (This can be worked-around with per-project
  extension/configuration, that `omp` will honor)
- It can run locally on a single developer's machine, within docker
automation, live on a shared VPS, etc; sky's the limit!
  - Being a Rust-based system, it aims for memory-efficiency; The _runtime_
  footprint (excluding `omp` sessions, but including any internal tools like
  memory, `rauthy`, etc) should be no more than two-to-three-hundred MB of RAM
    - `omp` has its own claims around memory-usage and can stand on its own
- In terms of the host Operating System: wherever it is running and whoever
it is running as will be the user/environment that `omprint` works within
  - `omprint` will bring `omp` and the footprint outlined below, but the rest (dev
  environment install, source control credential management, environment/
  secrets, etc) is the user's responsibility, which `omprint` works to
  remain ignorant-of
- As mentioned previously, the external tools/footprint that `omprint`
is responsible for fetching/installing/supervising:
  - A local, sandboxed install of `omp` to do all LLM work
  - An optional fetch/install of a sandboxed copy of [`rauthy`][5] (see below)
  - User(s) bring their own [provider configuration for `omp`][15], which
  `omprint` will persist internally
  - Packages/tools to enhance success rates and scale, like:
    - [Rust Token Killer][16]
    - [`mnemopi` memory-management][6]

### Differences from `bottega`

It strays from the [bottega reference][13] in several ways:

- `omprint` is a single-binary release, easily installable on any
system with _just_ `rustup` installed (and things it needs to build);
  - The implication is that `omprint` includes it's own vendored `omp`
    bin and does not rely on any install of `omp` outside of `omprint` itself;
    all packages, configuration, etc for `omp` is _interior_ to `omprint`,
    which supervises all instances of `omp` it utilizes
  - `omprint` has a "batteries included" philosophy of being richly featured,
  and keeping its capabilities _interior_ to the tool
    - It keeps it's footprint in a per-user (on the host OS) configuration/
    state directory
    - You may provide custom configuration for the _instance_ of `omprint`
    you run (whether locally, or on a VPS, or a hosted solution, etc)
- Tight coupling to the `omp` agent harness **only**
  - No Claude Code, no Codex, no OpenCode; There is only `omp`
  - This means users bring their own provider configuration for `omp`, and
  model choices fall out of from that
  - ..But everything else is about an orchestration system driving `oh-my-pi`
  sessions
- Reified as a [Rust][7]-based webapp, using the [leptos][3] framework;
`omprint` itself is an [axum][4]+[leptos][3] web-server that can run from
the CLI or be set up via a superviser system (e.g. [systemd][8])
- ⚠️**Requires OAuth2/OIDC for all [IAM][10]** ⚠️
  - `bottega` implements [its own authentication scheme][9] in the context
  of its reference implementation; _this is not appropriate_ for secure,
  production-ready deployments in an enterprise/organizational setting
  - `omprint` can be configured to either point at a well-known OAuth2/OIDC
  endpoint (where it will fetch the pub-cert for authenticating client requests
  on the server), or to install/run a self-hosted OAuth server (an audited tool
  named [`rauthy`][5])
- Out-of-the-box, per-project memory using `omp`'s [mnemopi][6] package
- Several subtle tweaks on _vanilla_ `bottega` that reflect the tastes
of `omprint`'s maintainership

## Installation

1. `cargo install --git https://github.com/olsonjeffery/omprint`
2. (FIXME: Guides!) Run it however you want (as a standalone dev server, via
systemd+VPS, as part of a k8s cluster, etc)

## Contributing

Like `bottega`, `omprint` is Specification First.

We maintain the `omprint` rust codebase as the de facto reference-
implementation of the spec.

Setting that aside, all `omprint` enhancements (besides outright bugfixes unrelated
to the specification) happen through refining & extending the [`omprint` spec][11].

## License

This repository is licensed & distributed under the terms of the [GNU AGPL][12].

[0]: https://omp.sh
[1]: https://ghuntley.com/loop/
[2]: https://vdaubry/bottega
[3]: https://www.leptos.dev/
[4]: https://github.com/tokio-rs/axum
[5]: https://github.com/sebadob/rauthy
[6]: https://github.com/can1357/oh-my-pi/tree/main/packages/mnemopi
[7]: https://rust-lang.org/
[8]: https://systemd.io/
[9]: https://github.com/vdaubry/bottega/blob/main/extra/auth-and-multi-user.md
[10]: https://en.wikipedia.org/wiki/Identity_and_access_management
[11]: ./spec/SPEC.md].
[12]: ./LICENSE
[13]: https://github.com/vdaubry/bottega/blob/main/SPEC.md
[14]: https://vdaubry.github.io/bottega-launch/
[15]: https://omp.sh/docs/providers
[16]: https://github.com/rtk-ai/rtk
