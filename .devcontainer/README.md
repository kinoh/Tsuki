# How to Coexist with a Git Worktree

Both sides need to reference each other using relative paths.

```
src/tsuki $ cat .git/worktrees/tsuki-work1/gitdir
../tsuki-work1/.git

src/tsuki-work1 $ cat .git
gitdir: ../tsuki/.git/worktrees/tsuki-work1
```
