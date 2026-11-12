// The camera: views that move as the hand does (docs/designs/camera/design.md, taste.md §3.10).
// The one file the app imports; README.md says how a view uses it. It imports nothing of the app's.

export * from "./numbers";
export { NO_EDGES, cull, fit, move, reveal, room, to, toBox, toContent, within, zoomAround } from "./math";
export type { Align, Camera, CullBy, Edges, Shape, Size, Span, Window } from "./math";
export { cameraNames, cameraOf, forgetCameras, hasCamera, onCamera, useCameraState, useCull, useZoomOf } from "./store";
export type { CameraState, MovedBy, Ways } from "./store";
export { controlsAt, controlsOf, useCamera, useNear } from "./useCamera";
export type { CameraOptions } from "./useCamera";
export type { CameraControls } from "./rig";
export { movingNow, pauseSwipes, setPlatform } from "./state";
export { useSpan } from "./spans";
export type { Platform } from "./state";
export { face } from "./face";
