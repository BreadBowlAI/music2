// The camera: views that move as the hand does (docs/designs/camera/design.md, taste.md §3.10).
// The one file the app imports; README.md says how a view uses it. It imports nothing of the app's.

export * from "./numbers";
export { NO_EDGES, cull, cullPlan, fit, move, reveal, room, to, toBox, toContent, within, zoomAround } from "./math";
export type { Align, Camera, CullBy, CullPlan, Edges, Shape, Size, Span, Window } from "./math";
export { CameraScope, cameraNames, cameraNow, onCamera, useCameraOf, useCameraState, useCull, useZoomOf } from "./store";
export type { CameraRef, CameraState, CameraStore, MovedBy, Ways } from "./store";
export { controlsAt, controlsOf, useCamera, useNear } from "./useCamera";
export type { CameraOptions } from "./useCamera";
export type { CameraControls } from "./rig";
export { onViewMoved } from "./rig";
export { movingNow, pauseSwipes, setPlatform } from "./state";
export { FINE, onPixels, spanBox, useSpan } from "./spans";
export type { Platform } from "./state";
export { face } from "./face";
