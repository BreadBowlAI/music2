// ci/xshot.c: `xshot <out.png>` saves the whole X screen of $DISPLAY as a PNG. ci/check.sh builds it
// on the fly (cc ci/xshot.c -lX11 -lpng) to keep a picture of the desktop app on its invisible Xvfb
// screen. It assumes a 24-bit TrueColor screen, as that Xvfb has.
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <png.h>
#include <stdio.h>
#include <stdlib.h>
int main(int argc, char **argv) {
  if (argc != 2) { fprintf(stderr, "usage: xshot <out.png>\n"); return 2; }
  Display *d = XOpenDisplay(NULL);
  if (!d) { fprintf(stderr, "xshot: cannot open display\n"); return 1; }
  Window root = DefaultRootWindow(d);
  XWindowAttributes a; XGetWindowAttributes(d, root, &a);
  XImage *img = XGetImage(d, root, 0, 0, a.width, a.height, AllPlanes, ZPixmap);
  if (!img) { fprintf(stderr, "xshot: XGetImage failed\n"); return 1; }
  FILE *f = fopen(argv[1], "wb");
  if (!f) { perror("xshot"); return 1; }
  png_structp p = png_create_write_struct(PNG_LIBPNG_VER_STRING, NULL, NULL, NULL);
  png_infop info = png_create_info_struct(p);
  if (setjmp(png_jmpbuf(p))) { fprintf(stderr, "xshot: png error\n"); return 1; }
  png_init_io(p, f);
  png_set_IHDR(p, info, a.width, a.height, 8, PNG_COLOR_TYPE_RGB, PNG_INTERLACE_NONE, PNG_COMPRESSION_TYPE_DEFAULT, PNG_FILTER_TYPE_DEFAULT);
  png_write_info(p, info);
  unsigned char *row = malloc(3 * a.width);
  for (int y = 0; y < a.height; y++) {
    for (int x = 0; x < a.width; x++) {
      unsigned long px = XGetPixel(img, x, y);
      row[3 * x] = (px & img->red_mask) >> 16;
      row[3 * x + 1] = (px & img->green_mask) >> 8;
      row[3 * x + 2] = px & img->blue_mask;
    }
    png_write_row(p, row);
  }
  png_write_end(p, NULL); fclose(f); XDestroyImage(img); XCloseDisplay(d);
  return 0;
}
