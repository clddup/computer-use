import { Ajv, type AnySchema, type ValidateFunction } from 'ajv';
import schema from './schema.json' with { type: 'json' };
export type { Action, ActionOutcome, Application, Capabilities, Element, Image, LaunchResult, Observation, Rect, Request, Response, Target, Window } from './generated.js';
export { schema };
export const VERSION = 1;
export const MAX_FRAME = 16 * 1024 * 1024;
const ajv = new Ajv({ strict: false, allErrors: true, formats: {
  double: { type: 'number', validate: Number.isFinite },
  uint32: { type: 'number', validate: (n: number) => Number.isInteger(n) && n >= 0 && n <= 0xffff_ffff },
  uint64: { type: 'number', validate: (n: number) => Number.isSafeInteger(n) && n >= 0 },
  int32: { type: 'number', validate: (n: number) => Number.isInteger(n) && n >= -0x8000_0000 && n <= 0x7fff_ffff },
} });
const validators = new Map<string, ValidateFunction>();
export function validator(name: string): ValidateFunction {
  let validate = validators.get(name);
  if (!validate) {
    validate = ajv.compile({ $schema: schema.$schema, definitions: schema.definitions, $ref: `#/definitions/${name}` } as AnySchema);
    validators.set(name, validate);
  }
  return validate;
}
export function assertWire<T>(name: string, value: unknown): T {
  const validate = validator(name);
  if (!validate(value)) throw new Error(`Invalid ${name}: ${ajv.errorsText(validate.errors)}`);
  return value as T;
}
export function compileInput(input: object): ValidateFunction {
  return ajv.compile({ ...input, definitions: schema.definitions } as AnySchema);
}
