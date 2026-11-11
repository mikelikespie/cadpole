// Everything the UI can ask the app to do. Implemented in main.ts; panels
// only receive this interface (plus the store) so they stay decoupled.

import type { WheelMode } from "./controls.js";
import type { EnginePreference } from "./engine-policy.js";
import type { LeftTab, SheetTab } from "./state.js";
import type { EntityRef, ParamValue, SourceSpan } from "./types.js";
import type { SectionState } from "./section.js";
import type { ViewMode, ViewPreset } from "./viewer.js";

export interface Actions {
  /** Replace (or, with additive, toggle into) the selection. */
  select(refs: EntityRef[], opts?: { additive?: boolean; announce?: boolean }): void;
  clearSelection(): void;
  hover(ref: EntityRef | null): void;
  setNote(note: string): void;

  /**
   * Set one parameter. `preview` is the slider still moving: the
   * evaluation it schedules is a drag preview, which the engine answers
   * with fuses deferred; the call without it is the full evaluation.
   */
  setOverride(name: string, value: ParamValue, preview?: boolean): void;
  resetOverride(name: string): void;
  resetAllOverrides(): void;

  sendMessage(text: string): Promise<void>;
  /** The Listen switch: pass what is typed to an attached assistant, or not. */
  setListening(listening: boolean): Promise<void>;
  loadExample(id: string): Promise<void>;
  applyScript(path: string, source: string): Promise<void>;
  save(): Promise<void>;

  /** Open the script viewer at a span (switches tab / sheet as needed). */
  revealSpan(span: SourceSpan): void;
  setLeftTab(tab: LeftTab): void;
  openSheet(tab: SheetTab): void;
  closeSheet(): void;
  toggleHidden(bodyId: string): void;

  /**
   * The Layers panel. Each of these touches only the viewer: no
   * evaluation, no message to the engine. `name` is a `Layer.name`, or ""
   * for the implicit "Other" layer of untagged bodies.
   */
  setLayerVisible(name: string, visible: boolean): void;
  /** 0..1. */
  setLayerOpacity(name: string, opacity: number): void;
  /** Show this layer alone; on the layer already soloed, show every layer again. */
  soloLayer(name: string): void;
  /** Back to what the script declared. */
  resetLayers(): void;

  setView(preset: ViewPreset): void;
  fit(): void;
  /** Fit the camera to the current selection (falls back to fit-all). */
  focusSelection(): void;
  setViewMode(mode: ViewMode): void;
  cycleViewMode(): void;
  /** shaded <-> shaded+edges (or back to shaded+edges from any other mode). */
  toggleEdges(): void;
  toggleGrid(): void;
  toggleLabels(): void;
  toggleBBox(): void;
  toggleOrtho(): void;
  toggleHud(): void;
  /**
   * The plates view: every body where `cadpole export` would put it on
   * its print plate, in its filament's colour. Only offered when the model
   * has plates; off puts the model view back exactly as it was.
   */
  setPlatesView(on: boolean): void;
  /**
   * The section: cut the view by up to one horizontal and one
   * vertical plane. Display only, like a layer: no evaluation.
   */
  setSection(section: SectionState): void;
  /** Open or close the Section panel; with no argument, toggle it. */
  toggleSectionPanel(open?: boolean): void;
  /** Put a link to the view on screen (camera, display mode, section) in the address bar and on the clipboard. */
  copyViewLink(): Promise<void>;
  /** What the camera frames, mm, for the Section panel's ranges; null before a model. */
  sceneBounds(): { min: [number, number, number]; max: [number, number, number] } | null;
  /** Frame one plate of `model.plates` (by index), or all of them (null). */
  setActivePlate(index: number | null): void;
  /** Capture the viewport to PNG and queue it for the next message. */
  snapshot(opts?: { annotate?: boolean }): Promise<void>;
  removeAttachment(index: number): void;
  setWheelMode(mode: WheelMode): void;
  wheelMode(): WheelMode;
  install(): Promise<void>;
  focusMessages(): void;

  /** Where evaluation runs: auto, this machine, or the server. */
  setEnginePreference(pref: EnginePreference): void;
  /**
   * Give up on the evaluation in flight.
   *
   * It leaves the displayed model exactly as it was: cancelling is "stop
   * computing", not "throw away what I am looking at".
   */
  cancelEvaluate(): void;

  /** Take a rebuilt bundle that needs a page load, now. */
  reloadForBuild(): void;
  /**
   * "Not now": collapse the offer to a chip.
   *
   * It never clears the fact that the page is behind, only the interruption.
   */
  dismissBuildUpdate(): void;
}
