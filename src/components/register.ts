import type { Component } from "vue";
import { registerComponent } from "../core/registry";
import CanvasLayout from "./builtins/CanvasLayout.vue";
import StackLayout from "./builtins/StackLayout.vue";
import RowLayout from "./builtins/RowLayout.vue";
import SectionLayout from "./builtins/SectionLayout.vue";
import CompositeLayout from "./builtins/CompositeLayout.vue";
import TextBlock from "./builtins/TextBlock.vue";
import ConnectionPanel from "./builtins/ConnectionPanel.vue";
import SimulationPanel from "./builtins/SimulationPanel.vue";
import CommandPanel from "./builtins/CommandPanel.vue";
import ScalarField from "./builtins/ScalarField.vue";
import StringField from "./builtins/StringField.vue";
import EnumField from "./builtins/EnumField.vue";
import ConfigTable from "./builtins/ConfigTable.vue";
import IgnitionTable from "./builtins/IgnitionTable.vue";
import ConfigCurve from "./builtins/ConfigCurve.vue";
import IniPanelsBrowser from "./builtins/IniPanelsBrowser.vue";
import OutputChart from "./builtins/OutputChart.vue";
import CompositeChart from "./builtins/CompositeChart.vue";
import OutputValue from "./builtins/OutputValue.vue";
import Dyno from "./builtins/Dyno.vue";
import KnockPanel from "./builtins/KnockPanel.vue";
import Spectrogram from "./builtins/Spectrogram.vue";
import ConfigChecklist from "./builtins/ConfigChecklist.vue";
import LuaScriptPanel from "./builtins/LuaScriptPanel.vue";
import IniCommandButton from "./builtins/IniCommandButton.vue";
import GeneratedPanel from "./builtins/GeneratedPanel.vue";
import ProjectTimeline from "./builtins/ProjectTimeline.vue";
import ProjectHistory from "./builtins/ProjectHistory.vue";
import ProjectScripts from "./builtins/ProjectScripts.vue";

let registered = false;

/** type → Vue SFC. Nav/container — в YAML инстанса panel. */
function reg(type: string, component: Component): void {
  registerComponent({ type, label: type, mode: "display", isContainer: false }, component);
}

function regCanvas(type: string, component: Component): void {
  registerComponent(
    { type, label: type, mode: "display", isContainer: true, handlesOwnChildren: true },
    component,
  );
}

export function registerBuiltinComponents(): void {
  if (registered) return;
  registered = true;

  regCanvas("canvas", CanvasLayout);
  reg("stack", StackLayout);
  reg("row", RowLayout);
  reg("section", SectionLayout);
  reg("composite", CompositeLayout);
  reg("text", TextBlock);
  reg("connection", ConnectionPanel);
  reg("simulation", SimulationPanel);
  reg("command", CommandPanel);
  reg("lua-script", LuaScriptPanel);
  reg("ini-command-button", IniCommandButton);
  reg("generated-panel", GeneratedPanel);
  reg("scalar-field", ScalarField);
  reg("string-field", StringField);
  reg("enum-field", EnumField);
  reg("config-table", ConfigTable);
  reg("ignition-table", IgnitionTable);
  reg("curve", ConfigCurve);
  reg("ini-panels-browser", IniPanelsBrowser);
  reg("output-chart", OutputChart);
  reg("composite-chart", CompositeChart);
  reg("output-value", OutputValue);
  reg("dyno", Dyno);
  reg("knock", KnockPanel);
  reg("spectrogram", Spectrogram);
  reg("config-checklist", ConfigChecklist);
  reg("project-timeline", ProjectTimeline);
  reg("project-history", ProjectHistory);
  reg("project-scripts", ProjectScripts);
}
