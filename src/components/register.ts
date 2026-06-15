import type { Component } from "vue";
import type { ComponentBindMeta } from "../core/types";
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

function reg(type: string, label: string, component: Component, bindMeta?: ComponentBindMeta): void {
  registerComponent({ type, label, mode: "display", isContainer: false, bindMeta }, component);
}

function regCanvas(type: string, label: string, component: Component): void {
  registerComponent(
    { type, label, mode: "display", isContainer: true, handlesOwnChildren: true },
    component,
  );
}

export function registerBuiltinComponents(): void {
  if (registered) return;
  registered = true;

  regCanvas("canvas", "Canvas Layout", CanvasLayout);
  reg("stack",   "Stack Layout",  StackLayout);
  reg("row",     "Row Layout",    RowLayout);
  reg("section", "Section",       SectionLayout);
  reg("composite", "Composite",   CompositeLayout);
  reg("text",    "Text Block",    TextBlock);

  reg("connection",  "Connection",         ConnectionPanel);
  reg("simulation",  "Simulation",         SimulationPanel);
  reg("command",     "Command Panel",      CommandPanel);
  reg("lua-script",  "Lua Script",         LuaScriptPanel);
  reg("ini-command-button", "INI Command Button", IniCommandButton);
  reg("generated-panel",    "Generated Panel",    GeneratedPanel);
  reg("ini-panels-browser", "INI Panels Browser", IniPanelsBrowser);
  reg("config-checklist",   "Config Checklist",   ConfigChecklist);

  reg("scalar-field", "Scalar Field", ScalarField,
    { autoSource: "config", needsConfigField: true });
  reg("string-field", "String Field", StringField,
    { autoSource: "config", needsConfigField: true });
  reg("enum-field",   "Enum Field",   EnumField,
    { autoSource: "config", needsConfigField: true });

  reg("config-table",    "Config Table",    ConfigTable,    { needsTable: true });
  reg("ignition-table",  "Ignition Table",  IgnitionTable,  { needsTable: true });
  reg("curve",           "Config Curve",    ConfigCurve,    { needsCurve: true });

  reg("output-chart",    "Log (Output Chart)",  OutputChart,
    { autoSource: "outputChannels" });
  reg("composite-chart", "Composite Chart",     CompositeChart,
    { autoSource: "compositeLogger" });
  reg("output-value",    "Output Value",        OutputValue,
    { autoSource: "outputChannels", needsOutputField: true });
  reg("dyno",            "Dyno",                Dyno,
    { autoSource: "outputChannels" });
  reg("knock",           "Knock Panel",         KnockPanel,
    { autoSource: "outputChannels" });
  reg("spectrogram",     "Spectrogram",         Spectrogram,
    { autoSource: "knockScope" });

  reg("project-timeline", "Project Timeline", ProjectTimeline);
  reg("project-history",  "Project History",  ProjectHistory);
  reg("project-scripts",  "Project Scripts",  ProjectScripts);
}
