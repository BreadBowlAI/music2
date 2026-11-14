#!/usr/bin/env python3
# The WebKit tool's window sizer (webkit.mjs; e2e/README.md, "WebKit numbers"): it resizes every top-level window
# on an X display to <width>×<height> px, at 0, 0, since no window manager runs there to do it.
#   DISPLAY=:N python3 -I xresize.py <width> <height>     prints each window's size before
import ctypes, ctypes.util, sys
x = ctypes.cdll.LoadLibrary(ctypes.util.find_library("X11"))
x.XOpenDisplay.restype = ctypes.c_void_p
x.XOpenDisplay.argtypes = [ctypes.c_char_p]
x.XDefaultRootWindow.restype = ctypes.c_ulong
x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
x.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)), ctypes.POINTER(ctypes.c_uint)]
x.XMoveResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint]
class Attrs(ctypes.Structure):
    _fields_ = [("x", ctypes.c_int), ("y", ctypes.c_int), ("width", ctypes.c_int), ("height", ctypes.c_int), ("border_width", ctypes.c_int), ("depth", ctypes.c_int), ("visual", ctypes.c_void_p), ("root", ctypes.c_ulong), ("class_", ctypes.c_int), ("bit_gravity", ctypes.c_int), ("win_gravity", ctypes.c_int), ("backing_store", ctypes.c_int), ("backing_planes", ctypes.c_ulong), ("backing_pixel", ctypes.c_ulong), ("save_under", ctypes.c_int), ("colormap", ctypes.c_ulong), ("map_installed", ctypes.c_int), ("map_state", ctypes.c_int), ("all_event_masks", ctypes.c_long), ("your_event_mask", ctypes.c_long), ("do_not_propagate_mask", ctypes.c_long), ("override_redirect", ctypes.c_int), ("screen", ctypes.c_void_p)]
x.XGetWindowAttributes.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(Attrs)]
w, h = int(sys.argv[1]), int(sys.argv[2])
d = x.XOpenDisplay(None)
root = x.XDefaultRootWindow(d)
r, p, kids, n = ctypes.c_ulong(), ctypes.c_ulong(), ctypes.POINTER(ctypes.c_ulong)(), ctypes.c_uint()
x.XQueryTree(d, root, ctypes.byref(r), ctypes.byref(p), ctypes.byref(kids), ctypes.byref(n))
for i in range(n.value):
    a = Attrs()
    x.XGetWindowAttributes(d, kids[i], ctypes.byref(a))
    if a.map_state == 2 and not a.override_redirect:  # viewable, a real top-level
        print(f"window {kids[i]:#x}: {a.width}x{a.height}")
        x.XMoveResizeWindow(d, kids[i], 0, 0, w, h)
x.XFlush.argtypes = [ctypes.c_void_p]
x.XFlush(d)
