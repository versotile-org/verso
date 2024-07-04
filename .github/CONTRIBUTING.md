# Contributing Guide

Hello! We, the maintainers, are very pleased that you are interested in contributing to Verso. However, before you submit your contribution, please take a moment to read the [Code of Conduct](CODE_OF_CONDUCT.md) and the appropriate section for the contribution you plan to make:

- [Issue Reporting Guidelines](#issue-reporting-guidelines)
- [Pull Request Guidelines](#pull-request-guidelines)

## Issue Reporting Guidelines

- The issue list on this repo is **exclusively** for bug reports and feature requests. Non-conforming issues will be closed immediately.

- If you have a question, you can get quick answers from the [Verso Zulip](https://versotile.zulipchat.com/).

- Try to search for your issue, it may have already been answered or even fixed in the main branch (`main`).

- Check if the issue is reproducible with the latest version of Verso. If you are using a nightly release, please indicate the specific version you are using.

- It is **required** that you clearly describe the steps necessary to reproduce the issue you are running into. Although we would love to help our users as much as possible, diagnosing issues without clear reproduction steps is extremely time-consuming and simply not sustainable.

- Use only the minimum amount of code necessary to reproduce the unexpected behavior. A good bug report should isolate specific methods that exhibit unexpected behavior and precisely define how expectations were violated. What did you expect the method or methods to do, and how did the observed behavior differ? The more precisely you isolate the issue, the faster we can investigate.

- Issues with no clear repro steps will not be triaged. If an issue labeled "need repro" receives no further input from the issue author for more than 5 days, it will be closed.

- If your issue is resolved but still open, don't hesitate to close it. In case you found a solution by yourself, it could be helpful to explain how you fixed it.

- Most importantly, we beg your patience: the team must balance your request against many other responsibilities — fixing other bugs, answering other questions, new features, new documentation, etc. The issue list is not paid support and we cannot make guarantees about how fast your issue can be resolved.

## Pull Request Guidelines

- It's OK to have multiple small commits as you work on the PR - we will let GitHub automatically squash it before merging.

- If adding a new feature:

  - Provide a convincing reason to add this feature. Ideally you should open a suggestion issue first and have it greenlighted before working on it.

- If fixing a bug:
  - If you are resolving a special issue, add `(fix: #xxxx[,#xxx])` (#xxxx is the issue id) in your PR title for a better release log, e.g. `fix: update entities encoding/decoding (fix #3899)`.
  - Provide detailed description of the bug in the PR, or link to an issue that does.

- The PR will require at least one approval from maintainers. In general, the CI workflows should pass as well, but it can bypass in edge cases or unusual scenarios.
