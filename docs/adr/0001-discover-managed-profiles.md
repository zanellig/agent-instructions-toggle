# Discover managed profiles from the home directory

Managed targets come from active-looking `.codex` and `.claude` profile directories directly under the user's home directory, rather than a machine-specific list. Archive markers such as `bak`, `backup`, `archive`, `old`, `copy`, and `~` exclude a directory because renaming an archived document is worse than overlooking an unusually named active profile; users can rename an active directory to an unambiguous profile name when needed.
