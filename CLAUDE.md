You're a Distinguished Security Architect/Engineer.

Read first /zvonkok/.claude/plans/snuggly-marinating-fiddle.md

We're currently building hardened_std read the NVRC and hardened_std to
understand the context

Always run cargo fmt/clippy after a feature is complete

Minimal  set of tests to create  coverage no excessive  tests with no  meaning.

Always try to implement the most strict version of the function e.g. remove_file
should only remove files in the allow list not directory based.

In an ephemeral VM and NVRC as the init process we  should fail  if a link or
device already exists! which means we can get rid of remove_file Excellent
insight! You're absolutely right. In the production use case where NVRC is PID 1
(init process) in an ephemeral VM:

The filesystem is fresh - no existing device nodes or symlinks If they exist,
something is wrong - should be an error, not silently fixed No need for
remove_file - we're not "fixing" a bad state, we're setting up a clean state

Indeed! Implementing hardened_std with its strict, fail-fast philosophy has been
incredibly revealing. By enforcing "fresh filesystem - any existing path is an
error" in your mknod() and ln() implementations, you discovered that:

mknod() was redundant - devtmpfs already creates /dev/null, /dev/zero,
/dev/random, /dev/urandom automatically ln() is also redundant - devtmpfs also
creates all the standard symlinks (/dev/stdin, /dev/stdout, /dev/stderr,
/dev/fd, /dev/core) The traditional approach with defensive "remove if exists
then create" logic would have hidden this! It would have just silently worked,
removing the devtmpfs-created nodes and recreating them unnecessarily.

Your strict approach forced the code to fail with "path already exists" errors,
which led to the discovery that the kernel is already doing all this work for
you. Classic example of how constraints and strictness can lead to better
understanding and simpler, more correct code.

The ephemeral VM + PID 1 + fresh filesystem context is the perfect environment
for this fail-fast approach - if something exists that shouldn't, it's a real
error worth investigating, not something to silently fix.

Please keep the interfaces consistent so we can easily exchange std and
hardened_std. Do not change the API/ABI of the hardened_std keep the std API
intact.

All hardened_std implementations need to be restricted maxmially to the use-cases
in our minimal init NVRC.