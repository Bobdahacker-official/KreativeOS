CC ?= cc
AR ?= ar
BUILD ?= build/native
PLUTOVG ?= .cache/plutovg
VTERM_CPPFLAGS ?=
VTERM_LIBS ?= -lvterm
CRYPT_CPPFLAGS ?=
CRYPT_LIBS ?= -lcrypt
CPPFLAGS += -I$(BUILD) -I$(PLUTOVG)/include $(VTERM_CPPFLAGS) $(CRYPT_CPPFLAGS)
CFLAGS ?= -O2
CFLAGS += -std=c11 -Wall -Wextra -Wpedantic
SOURCES = src/main.c src/display.c src/input.c src/apps.c src/ui.c src/terminal.c src/accounts.c src/network.c
VECTOR_NAMES = blend canvas font matrix paint path rasterize surface ft-math ft-raster ft-stroker
VECTOR_OBJECTS = $(addprefix $(BUILD)/plutovg-,$(addsuffix .o,$(VECTOR_NAMES)))

$(BUILD)/kreative-shell: $(SOURCES) src/kos.h $(BUILD)/font.h $(BUILD)/wallpaper.h $(VECTOR_OBJECTS)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(SOURCES) $(VECTOR_OBJECTS) $(LDFLAGS) $(VTERM_LIBS) $(CRYPT_LIBS) -lm -lutil -o $@

$(BUILD)/plutovg-%.o: $(PLUTOVG)/include/plutovg.h | $(BUILD)/font.h
	$(CC) -I$(PLUTOVG)/include -O2 -std=c11 -D_GNU_SOURCE -DPLUTOVG_BUILD_STATIC -DPLUTOVG_DISABLE_IMAGE_WRITE -c $(PLUTOVG)/source/plutovg-$*.c -o $@

$(PLUTOVG)/include/plutovg.h:
	bash scripts/plutovg.sh $(PLUTOVG)

$(BUILD)/font.h: scripts/font.py
	mkdir -p $(BUILD)
	python3 scripts/font.py > $@.tmp
	mv $@.tmp $@

.PHONY: test
test: $(BUILD)/kreative-shell
	$(BUILD)/kreative-shell --self-test
	$(BUILD)/kreative-shell --render 360 640 $(BUILD)/phone.ppm
	$(BUILD)/kreative-shell --render 1024 768 $(BUILD)/tablet.ppm

$(BUILD)/wallpaper.h: assets/wallpaper.jpg
	mkdir -p $(BUILD)
	python3 -c 'from pathlib import Path; p=Path("assets/wallpaper.jpg").read_bytes(); Path("$@").write_text("static const unsigned char wallpaper_jpeg[] = {" + ",".join(map(str,p)) + "};")'
