# VCS Nerd Cave

Use this path when evaluating object identity, Git bridge behavior, storage
layout, protocol compatibility, or edge cases.

## Start

- [Object format](../spec/object-format.md)
- [Object and protocol stability tiers](../reference/object-stability-tiers.md)
- [Data layout](../reference/data-layout.md)
- [Git interop workflow](../workflows/git-interop.md)
- [Benchmarks](../reference/benchmarks.md)
- [Compatibility](../reference/compatibility.md)

## Before trusting interop

Run and record the exact cases that matter for your repository: branches, tags,
notes, merge commits, renames, submodules, LFS pointers, binary files,
executable bits, symlinks, Unicode filenames, and large files. Keep Git as the
source of truth until those cases pass with the exact Claw version you intend to
run.
