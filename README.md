<p align="center">
  <img style="width:25%;height:25%;" src="assets/ofm-logo.png" />
</p>

<p align="center">
  <strong>Orchestration Force Multiplier (ofm)</strong>
  <strong><a href="https://github.com/olsonjeffery/ofm">GitHub repository</a></strong>
</p>

<p align="center">
  A specification-level fork of <a href="https://github.com/vdaubry/bottega">bottega</a> by <a href="https://github.com/vdaubry">@vdaubry</a> 
</p>

<p align="center">
    (Pronounce it as an acronym: oh-eff-em)
</p>

<p align="center">
  <a href="https://github.com/olsonjeffery/ofm/blob/main/LICENSE"><img src="https://img.shields.io/github/license/olsonjeffery/ofm?style=flat&colorA=222222&colorB=58A6FF" alt="License"></a>
  <a href="https://github.com/olsonjeffery/ofm/actions"><img src="https://img.shields.io/github/actions/workflow/status/olsonjeffery/ofm/ci.yml" alt="build status"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-DEA584?style=flat&colorA=222222&logo=rust&logoColor=white" alt="Rust"></a>
  <a href="https://deps.rs/repo/github/olsonjeffery/ofm"><img src="https://img.shields.io/deps-rs/repo/github/olsonjeffery/ofm" alt="deps.rs report on ofm"></a>
</p>

<p align="center">
  <strong><ins>ACHTUNG!<br />ACHTUNG!<br />ACHTUNG!</ins><br />⚠️ ofm is alpha-quality software ACHTUNG! ⚠️</strong>
</p>

<p align="center">
TL;DR An orchestration harness for coding agent activity, centering a kanban-like authoring lifecycle (especially for software).
</p>
<p align="center">
Think of it as a web-based layer, with a structured workflow, that mediates between User(s) and coding agents; it aims to increase productivity in the time and quality domains.
</p>

## Core attributes

### Capability 💪

- The system provides a more rigid structure around the [_Ralph Wiggum Loop_][1]
(hereafter referred to as _orchestration_ or simply _the loop_), helping users
to spend more time producing high-quality software, instead of fighting with the
agent harness
- Simultaneously, we don't want _too much structure_; that only stifles agility
and burns countless tokens on redundancy checks (looking at you, [opencode-swarm][18])
- An intuitive, web-based user interface creates an environment that enables
users to focus on defining requirements and providing needed feedback to LLM agents,
instead of thrashing with tooling or environment setup
- `playwright-cli` comes out of the box as an agent capability

### Visibility 👁️

- `ofm` preserves logs of agent activity it drives; Full JSON export and import is
supported
- All prompts are surfaced and auditable; no secret sauce or dumbing-down for users
- The web-based user interface and kanban style task board provides at-a-glance
snapshots of current progress; the task-level view highlights points of interactivity
or needed user intervention to get a coding agent back on-track

### Flexibility ♾️

- All prompts can be changed on a global, per-project and/or per-user
basis
- A choice between [`oh-my-pi`][0] and [`opencode`][17] (two open-source, multi-provider
capable coding agent harnesses) allows the user to use different approaches where
warranted
- The user _owns_ the local installations of the coding agents, so they can customize
then with whatever skills, safeguards, etc are appropriate for the user case or
organizational requirements; This avenue of customization provides a positive feedback
loop into the Capability attribute

## Why AGPL 3.0 for the license?

> ℹ️`ofm` has plans to develop an out-of-process extension mechanism, so
> organizations that need proprietary integration can place such code
> within a unit that sits outside of the AGPL 3.0 boundary, but integrates
> seamlessly with `ofm`

`ofm` is [Free Software][12] in the purest sense of the term: It cannot be taken
closed source _by anyone_ (including the founding author); It can be productized,
yet all changes must be contributed back into the public repository for the benefit
of all.

Any organization who adopts the system internally has nothing to fear from the
license terms. They are meant to discourage productization without contributing
back to the upstream.

Opening issues and contributing is encouraged for those wishing to extend the
core/optional capabilities of `ofm`.

## Installation

At this time, `ofm` is started/ran by cloning this repository, then executing:

```bash
cargo build --release && <NEEDED-OAUTH-ENV-VARS> ./target/release/ofm
```

> ℹ️In the future there will likely be a docker image distro, along with possibly
> a [crates.io][19] bin release.

Note the placement of `<NEEDED-OAUTH-ENV-VARS>` in the above snippet; on first
run, the installing individual will want to indicate which OAuth provider
should be used: Either an external OAuth provider, or the integrated [rauthy][5]
provider.

**Rauthy users can simply provide:**

```bash
# This value drives ofm using docker to run the latest rauthy
# img released on ghcr.io
OFM_RAUTHY_ENABLED=true
```

...as a prefix to the `./target/release/ofm` command.

> ℹ️**NOTE FOR RAUTHY USERS**: On first run, the console output will include
> an admin password generated by rauthy for the initial user (username
> `admin@localhost`); This password must be captured and used to do an initial
> login; After that, the user can change their password by going to the "User
> Settings" page linked from the `ofm` top navbar)

**Installations using an OAuth provider will want to provide:**

```bash
# This is the OAuth "base" (i.e. .well-known/openid-configuration)
# should be *beneath* this URL
OFM_OIDC_ISSUER_URL=https://path.to/oauth/issuer-base
# This is the client used in the web application for OIDC; it should
# be configured for Code Authorization flow and PKCE
OFM_OIDC_CLIENT_ID=ofm.client
```

Either of these approaches will initialize the `ofm` footprint at `$HOME/.ofm`
by default (provide `OFM_FOOTPRINT={path}` to customize this).

In either case: after the first run of `ofm`, this OAuth preference will be
persisted in `$OFM_FOOTPRINT/config/ofm.yml`, so future runs of `ofm` will
not need to provide this (unless the user has a custom `OFM_FOOTPRINT` location;
then that env var should be set on every run of `ofm`).

At this point, you should have a server bounding to `0.0.0.0` and reachable at
`localhost:3183` running on your machine (`3183` is the default port; Set the `OFM_PORT`
environment variable if you wish for it to run on another port).

> ℹ️`ofm` itself _does not_ consider running with a certificate/TLS+SSL as in-scope.
> It is also recommend, if planning to expose `ofm` on the public internet, to
> place `ofm` behind a reverse proxy such as `nginx`/`haproxy` etc and doing SSL
> termination there.
>
> If encrypted traffic is mandatory within your organization, then `ofm` should
> have the enclosing reverse proxy as an on-machine sidecar, with the `ofm` ports
> blocked by a software firewall for non-localhost users.

## History & evolution

### The `bottega` method

`ofm` is descended from [`vdaubry/bottega`][2], which means it is
_task-driven_. What does this mean? From the [bottega announcement][14]:

> A task is not a prompt. A task is a requirement with acceptance criteria.
>
> The task itself, the requirement, and the technical specification must all
> coexist as enduring artifacts that live alongside the implementation, not
> transient inputs to a single session.

This philosphy colors how `bottega` & `ofm` organize, present and execute
work on behalf of its users. Note that these tasks, and their artifacts,
exist separately from any specification living within the codebase (this
applies to `ofm`).

Additionally:

- Tasks, memory and related documentation live **outside** of code repositories
and worktrees `ofm` is used on
- It's implementation is specification-based; everything starts at
`specs/SPEC.md`; read this to begin understanding _how_ `ofm` works and
what is in-scope
- It is a _web-based_ system, with limited CLI capabilities for onboarding and
agent tools only
- It is _multi-user_ and _persistent_ by design; It is meant for teams
that cooperate to ship software (it is also a pleasant system to run as a
solo programmer (the [Core attributes](#core-attributes) described above
articulate this more fully)
  - Provider configuration can be global and/or per-user
- It can run locally on a single developer's machine, within docker
automation, live on a shared VPS, etc; sky's the limit!
  - Being a Rust-based system, it aims for memory-efficiency; The _runtime_
  footprint (excluding agent sessions, but including any internal tools like
  memory, `rauthy`, etc) should be no more than two-to-three-hundred MB of RAM
    - supported agents has their own claims around memory-usage and can stand
    on their own
- In terms of the host Operating System: wherever it is running and whoever
it is running-as will be the user/environment that `ofm` works within
  - `ofm` has a _data footprint_ as well as its _system dependencies_
  (installed tools that `ofm` expects to be installed and available to the
  user)
  - apart from what's above, the rest (dev environment install, source control
  credential management, environment/secrets, etc) is the user's responsibility,
  which `ofm` works to remain ignorant-of

### Differences from `bottega`

It strays from the [bottega reference][13] in several ways:

- `ofm` is a single-binary release; It has a list of needed dependencies
in order to be _useful_, but the system itself is self-contained
- Reified as a [Rust][7]-based webapp, using the [leptos][3] framework;
`ofm` itself is an [axum][4]+[leptos][3] web-server that can run from
the CLI or be set up via a superviser system (e.g. [systemd][8])
- ⚠️**Requires OAuth2/OIDC for all [IAM][10]** ⚠️
  - `bottega` implements [its own authentication scheme][9] in the context
  of its reference implementation; _this is not appropriate_ for secure,
  production-ready deployments in an enterprise/organizational setting
  - `ofm` can be configured to either point at a well-known OAuth2/OIDC
  endpoint (where it will fetch the pub-cert for authenticating client requests
  on the server), or to install/run a self-hosted OAuth server (an audited tool
  named [rauthy][5])
- Several subtle tweaks on _vanilla_ `bottega` that reflect the tastes
of `ofm`'s founding author

## Contributing

### The `ofm` specification

**TL;DR:** All changes must include updates within the content of the `spec/`
directory in the root of this repository. It is often preferred for PRs/issues
to be articulated in terms of updates to the specification.

Like `bottega`, `ofm` is Specification First.

We maintain the `ofm` rust codebase as the de facto reference-implementation of
the spec.

Setting that aside, all `ofm` enhancements (besides outright bugfixes unrelated
to the specification) happen through refining & extending the [`ofm` spec][11].

### Vouching

`ofm` uses the same **vouching** scheme as `oh-my-pi`. See [CONTRIBUTING.md](./CONTRIBUTING.md)
for details.

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
[11]: ./spec/SPEC.md
[12]: ./LICENSE
[13]: https://github.com/vdaubry/bottega/blob/main/SPEC.md
[14]: https://vdaubry.github.io/bottega-launch/
[15]: https://omp.sh/docs/providers
[16]: https://github.com/rtk-ai/rtk
[17]: https://opencode.ai
[18]: https://github.com/ZaxbyHub/opencode-swarm/
[19]: https://crates.io
