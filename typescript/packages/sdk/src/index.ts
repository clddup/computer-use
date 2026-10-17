// Agent-neutral entry point: no LLM, host evaluation, shell execution, or provider coupling.
export { ComputerRuntime as Computer, ComputerRuntime, ComputerUseError, UnknownExecutionError } from '@independent-cu/runtime';
export type { HelperOptions } from '@independent-cu/runtime';
export type { Action, ActionOutcome, Application, Capabilities, Element, Image, LaunchResult, Observation, Rect, Target, Window } from '@independent-cu/protocol';
