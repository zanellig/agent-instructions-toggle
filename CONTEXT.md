# Agent instruction toggle

Controls whether global instruction documents are discoverable when a coding agent creates a new context.

## Language

**Instruction document**:
A global document whose recognized filename causes a coding agent to add its contents to a new context.
_Avoid_: Prompt file, rules file

**Managed target**:
An instruction document controlled by this tool, including its recognized and disabled names.
_Avoid_: Watched file

**Instruction state**:
The aggregate state derived from all managed targets: `on`, `off`, `mixed`, or `conflict`.
_Avoid_: Toggle state, injection state

**Mixed state**:
Some managed targets are on and others are off, with no target containing both names.
_Avoid_: Conflict, partial state

**Conflict**:
A state where a managed target contains both names, or no managed targets remain from which to derive an instruction state.
_Avoid_: Mixed state

**New context**:
A coding-agent context created after an instruction-state transition. Existing contexts retain the instructions they already loaded.
_Avoid_: Next turn
