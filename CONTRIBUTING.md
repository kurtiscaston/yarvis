# Contributing to Yarvis

Contributions are welcome: bug reports, fixes, themes and features.

## Before you start

For anything larger than a small fix, open an issue first and describe what
you want to change. It saves you building something that will not be merged.

## The license and your contribution

Yarvis is source-available under the [Sustainable Use License](LICENSE.md).
Anyone may use and modify it, and share it free of charge for non-commercial
purposes, but only the owner may sell it.

So that the owner can include your work in everything he does with Yarvis,
including selling it or changing its license later, your first pull request
needs your agreement to the [contributor license agreement](CLA.md). You keep
ownership of your contribution. Tick the box in the pull request template to
agree. A pull request without that agreement cannot be merged.

## Working on the code

Set up and run the app as described in the [README](README.md). Before you
open a pull request, run:

```
npm test         # Rust unit tests
npm run check    # type-check the UI
```

Keep each pull request to one change, and say in its description what you
changed and how you checked it.
