/* libvterm behind a flat C interface for the comparison adapter.
 *
 * libvterm keeps no scrollback: it hands each line that scrolls off the top to a callback.
 * Every embedder must store those lines, so the adapter does too, in a ring allocated once
 * when the terminal is created. A pushed line is one memcpy into its slot: no per-cell work
 * and no per-line allocation. This cost is timed and the engine is labelled "+ring". */

#include <stdlib.h>
#include <string.h>

#include "vterm.h"

typedef struct {
  VTerm *vt;
  VTermScreen *screen;
  VTermScreenCell *ring; /* capacity rows of cols cells, allocated once */
  int cols;
  size_t capacity;
  size_t head;  /* slot the next pushed line goes to */
  size_t count; /* lines held, at most capacity */
} Lv;

typedef struct {
  uint32_t chars[VTERM_MAX_CHARS_PER_CELL];
  int width;
  int bold, italic, underline, reverse;
  int fg_kind, bg_kind; /* 0 default, 1 palette index, 2 rgb */
  uint8_t fg[3], bg[3]; /* palette index in [0], or r g b */
} LvCell;

static int push_line(int cols, const VTermScreenCell *cells, void *user) {
  Lv *lv = (Lv *)user;
  if (lv->capacity == 0) return 1;
  memcpy(lv->ring + lv->head * (size_t)lv->cols, cells, (size_t)cols * sizeof(VTermScreenCell));
  lv->head = (lv->head + 1) % lv->capacity;
  if (lv->count < lv->capacity) lv->count++;
  return 1;
}

static int pop_line(int cols, VTermScreenCell *cells, void *user) {
  (void)cols; (void)cells; (void)user;
  return 0; /* the grid never grows, so libvterm never asks for lines back */
}

static int clear_scrollback(void *user) {
  Lv *lv = (Lv *)user;
  lv->count = 0;
  lv->head = 0;
  return 1;
}

static const VTermScreenCallbacks callbacks = {
  NULL, NULL, NULL, NULL, NULL, NULL, push_line, pop_line, clear_scrollback, NULL,
};

Lv *lv_new(int rows, int cols, size_t scrollback) {
  Lv *lv = (Lv *)calloc(1, sizeof(Lv));
  if (!lv) return NULL;
  lv->cols = cols;
  lv->capacity = scrollback;
  lv->ring = (VTermScreenCell *)calloc(scrollback * (size_t)cols + 1, sizeof(VTermScreenCell));
  lv->vt = vterm_new(rows, cols);
  if (!lv->ring || !lv->vt) {
    if (lv->vt) vterm_free(lv->vt);
    free(lv->ring);
    free(lv);
    return NULL;
  }
  vterm_set_utf8(lv->vt, 1);
  lv->screen = vterm_obtain_screen(lv->vt);
  vterm_screen_enable_altscreen(lv->screen, 1);
  vterm_screen_set_callbacks(lv->screen, &callbacks, lv);
  vterm_screen_reset(lv->screen, 1);
  return lv;
}

void lv_free(Lv *lv) {
  if (!lv) return;
  vterm_free(lv->vt);
  free(lv->ring);
  free(lv);
}

size_t lv_write(Lv *lv, const char *bytes, size_t len) {
  return vterm_input_write(lv->vt, bytes, len);
}

void lv_cursor(Lv *lv, int *row, int *col) {
  VTermPos pos;
  vterm_state_get_cursorpos(vterm_obtain_state(lv->vt), &pos);
  *row = pos.row;
  *col = pos.col;
}

static void export_color(const VTermColor *color, int is_default, int *kind, uint8_t out[3]) {
  if (is_default) {
    *kind = 0;
  } else if (VTERM_COLOR_IS_INDEXED(color)) {
    *kind = 1;
    out[0] = color->indexed.idx;
  } else {
    *kind = 2;
    out[0] = color->rgb.red;
    out[1] = color->rgb.green;
    out[2] = color->rgb.blue;
  }
}

static void export_cell(const VTermScreenCell *cell, LvCell *out) {
  memset(out, 0, sizeof(*out));
  memcpy(out->chars, cell->chars, sizeof(out->chars));
  out->width = cell->width;
  out->bold = cell->attrs.bold;
  out->italic = cell->attrs.italic;
  out->underline = cell->attrs.underline != 0;
  out->reverse = cell->attrs.reverse;
  export_color(&cell->fg, VTERM_COLOR_IS_DEFAULT_FG(&cell->fg), &out->fg_kind, out->fg);
  export_color(&cell->bg, VTERM_COLOR_IS_DEFAULT_BG(&cell->bg), &out->bg_kind, out->bg);
}

void lv_cell(Lv *lv, int row, int col, LvCell *out) {
  VTermPos pos = { row, col };
  VTermScreenCell cell;
  if (!vterm_screen_get_cell(lv->screen, pos, &cell)) {
    memset(out, 0, sizeof(*out));
    return;
  }
  export_cell(&cell, out);
}

size_t lv_history_len(const Lv *lv) { return lv->count; }

/* Cell `col` of the `index`-th held line, oldest first. */
void lv_history_cell(const Lv *lv, size_t index, int col, LvCell *out) {
  size_t oldest = (lv->head + lv->capacity - lv->count) % lv->capacity;
  size_t slot = (oldest + index) % lv->capacity;
  export_cell(lv->ring + slot * (size_t)lv->cols + (size_t)col, out);
}
