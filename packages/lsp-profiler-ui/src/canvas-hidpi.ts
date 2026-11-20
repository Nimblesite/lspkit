// Implements [PROF-UI-WEBVIEW]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * One device-pixel-ratio scaler for every profiler canvas webview: the memory
 * dashboard timeline, the flame graph and the retention graph. A Retina screen
 * renders two (or three) device pixels per CSS pixel, so a canvas that sizes
 * its backing store in CSS pixels draws blurred.
 *
 * The webview scripts embed this function's COMPILED SOURCE via
 * Function.prototype.toString, so the exact code every webview executes is
 * type-checked and unit-tested here. The rules of flame-script-helpers apply,
 * plus one the CommonJS output forces: no module-level constants (an exported
 * const compiles to `exports.X`, which does not exist in the webview).
 */

/** The canvas transform matrix, as the 2D context's setTransform takes it. */
export type TransformMatrix = readonly [number, number, number, number, number, number];

/** The canvas surface the scaler writes: CSS size and backing store. */
export interface DevicePixelCanvas {
  style: { width: string; height: string };
  width: number;
  height: number;
}

/** A 2D context whose transform the scaler sets. */
export interface ScaledCanvasContext {
  setTransform: (...matrix: TransformMatrix) => void;
}

/** What the scaler sizes: the canvas, its context, its CSS size and the screen's ratio. */
export interface HiDpiRequest {
  canvas: DevicePixelCanvas;
  ctx: ScaledCanvasContext;
  cssWidth: number;
  cssHeight: number;
  devicePixelRatio: number;
}

/**
 * Size a canvas for its screen: the CSS size in logical pixels, the backing
 * store multiplied by `devicePixelRatio`, and the context scaled by the same
 * ratio so all later drawing keeps using CSS-pixel coordinates.
 */
export function sizeCanvasToDevicePixels(request: HiDpiRequest): void {
  const ratio = request.devicePixelRatio > 0 ? request.devicePixelRatio : 1;
  request.canvas.style.width = `${request.cssWidth}px`;
  request.canvas.style.height = `${request.cssHeight}px`;
  request.canvas.width = Math.round(request.cssWidth * ratio);
  request.canvas.height = Math.round(request.cssHeight * ratio);
  request.ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
}
