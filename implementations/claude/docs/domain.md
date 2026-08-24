# Claude implementation terms

## Language

**Agent home**:
A directory under `$HOME` whose name marks it as a coding agent's configuration root. Managed targets are derived from the agent homes found by discovery, never from a fixed list.
_Avoid_: Profile directory, config directory

**Backup profile**:
An agent home whose name carries an archive word such as `backup`, `bak`, or `old`. Discovery finds it, reports it, and never modifies it.
_Avoid_: Excluded target, ignored file
