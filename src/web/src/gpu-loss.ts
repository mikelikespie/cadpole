// A graphics device that went away mid-frame.
//
// When a WebGPU device or its instance is dropped, the renderer's pending
// calls reject (`popErrorScope`, `mapAsync`, `onSubmittedWorkDone`) and
// nobody awaits them: they surface as unhandled rejections, often before, or
// instead of, the device's own `lost` promise. The viewer treats one as the
// device being lost and brings the view back; the failure reporter leaves it
// to the viewer rather than reporting a page error.

/** True when an unhandled rejection is a graphics device going away. */
export function isGraphicsDeviceLoss(reason: unknown): boolean {
  const message = reason instanceof Error || (typeof DOMException !== "undefined" && reason instanceof DOMException) ? (reason as Error).message : typeof reason === "string" ? reason : "";
  return /Instance dropped|device (?:is |was )?(?:lost|destroyed)|popErrorScope|GPUDevice.*lost/i.test(message);
}
