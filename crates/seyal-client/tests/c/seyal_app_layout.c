#include "SeyalApp.h"

#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

#define REQUIRE(cond)                                                          \
    do {                                                                       \
        if (!(cond)) {                                                         \
            fprintf(stderr, "layout check failed: %s\n", #cond);               \
            return 1;                                                          \
        }                                                                      \
    } while (0)

int main(void) {
    REQUIRE(SEYAL_APP_ABI_VERSION == 1);
    REQUIRE(sizeof(SeyalAppAction) == 120);
    REQUIRE(offsetof(SeyalAppAction, version) == 0);
    REQUIRE(offsetof(SeyalAppAction, payload) == 104);
    REQUIRE(sizeof(SeyalAppSnapshot) == 112);
    REQUIRE(offsetof(SeyalAppSnapshot, output_utf8) == 80);
    REQUIRE(offsetof(SeyalAppSnapshot, recovery_generation) == 104);
    REQUIRE(sizeof(SeyalAppAxNode) == 72);
    REQUIRE(sizeof(SeyalAppAccessibility) == 24);
    REQUIRE(sizeof(SeyalAppComposer) == 40);
    REQUIRE(sizeof(SeyalAppChrome) == 24);
    REQUIRE(sizeof(SeyalAppTheme) == 16);
    REQUIRE(sizeof(SeyalAppShell) == 64);
    REQUIRE(sizeof(SeyalAppRow) == 112);
    REQUIRE(offsetof(SeyalAppRow, address_version) == 56);
    REQUIRE(offsetof(SeyalAppRow, address_bytes) == 64);
    REQUIRE(SEYAL_APP_ACTION_MOVE_SPLIT_DIVIDER == 58);
    REQUIRE(SEYAL_APP_ACTION_TERMINATE_EXECUTION == 59);
    REQUIRE(SEYAL_APP_ACTION_NAVIGATE == 60);
    REQUIRE(sizeof(SeyalAppPaneRegion) == 40);
    REQUIRE(offsetof(SeyalAppPaneRegion, x) == 24);
    REQUIRE(sizeof(SeyalAppPaneDivider) == 56);
    REQUIRE(offsetof(SeyalAppPaneDivider, line_x) == 40);
    REQUIRE(offsetof(SeyalAppPaneDivider, ratio) == 48);
    REQUIRE(sizeof(SeyalAppAction) != 0);
    puts("seyal_app_layout ok");
    return 0;
}
