/** Типы с реализацией логики в Rust (`rusefui-runtime`). Должны совпадать с LogicComponentType. */
const RUST_LOGIC_TYPES = new Set([
  "connection",
  "simulation",
  "dyno",
  "knock",
  "config-table",
  "ignition-table",
  "command",
  "lua-script",
]);

export function requiresRustLogic(componentType: string): boolean {
  return RUST_LOGIC_TYPES.has(componentType);
}
