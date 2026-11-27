CC ?= cc
BUILD ?= build/native
CPPFLAGS += -I$(BUILD)
CFLAGS ?= -O2
CFLAGS += -std=c11 -Wall -Wextra -Wpedantic
SOURCES = src/main.c src/display.c src/input.c src/apps.c src/ui.c

$(BUILD)/kreative-shell: $(SOURCES) src/kos.h $(BUILD)/font.h
	$(CC) $(CPPFLAGS) $(CFLAGS) $(SOURCES) $(LDFLAGS) -lm -o $@

$(BUILD)/font.h: scripts/font.py
	mkdir -p $(BUILD)
	python3 scripts/font.py > $@.tmp
	mv $@.tmp $@

.PHONY: test
test: $(BUILD)/kreative-shell
	$(BUILD)/kreative-shell --self-test
	$(BUILD)/kreative-shell --render 360 640 $(BUILD)/phone.ppm
	$(BUILD)/kreative-shell --render 1024 768 $(BUILD)/tablet.ppm
