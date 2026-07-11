<p align="center">
  <img style="width:25%;height:25%;" src="assets/ofm-logo.svg" />
</p>

# Orchestration Force Multiplier (`ofm`)

This is the repository for `ofm`: An orchestration harness, for agentic code delivery. Think of it as a meta-system that sits atop your coding agent to make you even more productive.

(Pronouce it as an acronym: oh-eff-em)

## Core Qualities

### Capability 💪

- The system provides a more rigid structure around the [_Ralph Wiggum Loop_][1]
(hereafter referred to as _the loop_), helping agentic coders to spend more time
producing high-quality software, instead of fighting with the agent harness
- Simultaneously, we don't want _too much structure_; that only stifles productivity
and burns countless tokens on redundency checks (looking at you, [opencode-swarm][18])
- An intuitive, web-based user interface creates an environment that let's
Users focus on defining requirements and providing needed feedback to LLM agents,
instead of thrashing with tooling or environment setup
- `playwright-cli` comes out of the box as an agent enhancement

### Visibility 👁️
- `ofm` preserves logs of agent activity it drives
- All prompts are surfaced and auditable; no secret sauce or dumbing-down for Users
- The web-based user interface and kanban style task board provides at-a-glance
snapshots of current progress, highlighting points of interactivity or needed User
intervention to get a coding agent back on-track

### Flexibility ♾️

- All prompts can be changed on a global, per-project and/or per-user
basis
- A choice between [`oh-my-pi`][0] and [`opencode`][17] (two open-source,
multi-provider capable coding agent harnesses) allows the user to use different
approaches where warranted
- Multiple points of extensibility to build out capabilities within coding agents,
providing a positive feedback loop into the Capability core value
- `ofm` is [Free Software][12] in the purest sense of the term: It cannot be taken
closed source _by anyone_ (including the founding author); It can be productized,
yet all changes must be contributed back into the public repository for the benefit
of all

## Additional values

### The `bottega` method

`ofm` is descended from [`vdaubry/bottega`][2], which means it is
_task-driven_. What does this mean? From the [bottega announcement][14]:

> A task is not a prompt. A task is a requirement with acceptance criteria.
>
> The task itself, the requirement, and the technical specification must all
> coexist as enduring artifacts that live alongside the implementation, not
> transient inputs to a single session.

This philosphy colors how `bottega` & `ofm` organize, present and execute
work on behalf of its users.

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
solo programmer; It provides safety, durability, auditability, and more)
  - Provider configuration can be global and/or per-user
- It can run locally on a single developer's machine, within docker
automation, live on a shared VPS, etc; sky's the limit!
  - Being a Rust-based system, it aims for memory-efficiency; The _runtime_
  footprint (excluding agent sessions, but including any internal tools like
  memory, `rauthy`, etc) should be no more than two-to-three-hundred MB of RAM
    - supported agents has their own claims around memory-usage and can stand
    on their own
- In terms of the host Operating System: wherever it is running and whoever
it is running as will be the user/environment that `ofm` works within
  - `ofm` has a _data footprint_ as well as its _system dependencies_
  (installed tools that `ofm` expects to be installed and available to the
  user)
  - apart from what's above, the rest (dev environment install, source control
  credential management, environment/secrets, etc) is the user's responsibility,
  which `ofm` works to remain ignorant-of

### Differences from `bottega`

It strays from the [bottega reference][13] in several ways:

- `ofm` is a single-binary release, easily installable on any
system with _just_ `rustup` installed (and things it needs to build);
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
  named [`rauthy`][5])
- Several subtle tweaks on _vanilla_ `bottega` that reflect the tastes
of `ofm`'s maintainership

## Contributing

Like `bottega`, `ofm` is Specification First.

We maintain the `ofm` rust codebase as the de facto reference-
implementation of the spec.

Setting that aside, all `ofm` enhancements (besides outright bugfixes unrelated
to the specification) happen through refining & extending the [`ofm` spec][11].

### Vouching scheme

`ofm` uses a [vouching system][19] to manage access to the repo. It's purpose
is to prevent "drive-by"/spam Pull Requests. Especially those from automated
processes. Simply:

- A 'vouch' is applied at the github user level, given to the target user by
another user with github contributor status, usually in an issue/PR comment
- The 'vouch list' is tracked as VOUCHED.md in the root of this repository
- An issue opened by a user without a vouch is immediately flagged `Unvouched`;
contributors review the queue of unvouched issues daily
- A PR opened by an unvouched user will be closed immediately
- A PR opened by a vouched user will be allowed to remain and get reviewed,
and (hopefully!) merged
- Unvouched users who repeatedly open PRs will receive a ban from interacting
with the repository
- Vouches can be removed by any contributor; The `VOUCHED.md` file also maintains
a "shitlist" of users who're either known, by the community, untrustworthy individuals
OR those who have lost their vouch for other reasons
- Contributors themselves do not have direct commit permissions; they must also
go through the PR process for all changes
- A github action monitoring comments in issues and PRs is responsible for
modifying the VOUCHED.md file as-needed with direct commits to `main`

The recommended workflow is that:

1. an unvouched User should open an issue discussing what they want, and if they
want to open a PR for it
2. After discussion with contributors acting as mentors, the unvouched user may
receive a `vouch @username` comment from a contributor. This makes them eligible
to open a PR now and going forward
3. They open a PR, adhering to community and contributor guidelines; provided they
abide by the norms of the community and its code of conduct, they can retain their
vouched status indefinitely, and even earn contributor status themselves


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
[11]: ./spec/SPEC.md
[12]: ./LICENSE
[13]: https://github.com/vdaubry/bottega/blob/main/SPEC.md
[14]: https://vdaubry.github.io/bottega-launch/
[15]: https://omp.sh/docs/providers
[16]: https://github.com/rtk-ai/rtk
[17]: https://opencode.ai
[18]: https://github.com/ZaxbyHub/opencode-swarm/
