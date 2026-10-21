# Governance

**In short**
- music2 has one maintainer today: the project's owner.
- The owner decides the goals, the taste and the contract. Everyone can propose a change to any of them.
- Pull requests are reviewed and merged by the maintainer.

## Who decides what

| What | Where it is written | Who decides |
|---|---|---|
| The goals, non-goals and principles | [docs/goals.md](docs/goals.md) | the owner |
| The taste: how the app looks and feels | [docs/designs/step8/taste.md](docs/designs/step8/taste.md) | the owner |
| The contract: the rules every part is built and reviewed against | [docs/architecture.md](docs/architecture.md) | the owner |
| Which pull requests are merged | GitHub | the maintainer |
| These rules | this page | the owner |

## How to propose a change

- **A small change** (a fix, a gap from [docs/gaps.md](docs/gaps.md)): open an issue, or claim one, then send a pull request. [CONTRIBUTING.md](CONTRIBUTING.md) says how.
- **A big change** (a new feature, a new rule, a change to the format or the SDK): open an issue first, and write a design note under [docs/designs/](docs/designs/README.md) before the code. The owner says yes or no in the issue.
- **A change to a goal, the taste or the contract:** open an issue that says what to change and why. If the owner agrees, the page changes first: in the same pull request as any code that needs it, in a commit ahead of that code.

## More maintainers

The owner may add maintainers later, and will list them here with what each may decide.

## Conduct

Everyone in the project follows the [Code of Conduct](CODE_OF_CONDUCT.md). The maintainer enforces it.
