/* Pure recovery policy: a kernel-origin fault must NEVER be swallowed. */
#include <indianos/user_fault.h>
#include <assert.h>
#include <stdio.h>

int main(void) {
    assert(indianos_user_fault_eligible(14, 0x2b, 4, true));
    assert(indianos_user_fault_eligible(14, 0x2b, 5, true));
    assert(indianos_user_fault_eligible(13, 0x2b, 0, true));
    assert(indianos_user_fault_eligible(6, 0x2b, 0, true));
    assert(!indianos_user_fault_eligible(14, 0x2b, 12, true));
    assert(!indianos_user_fault_eligible(14, 0x08, 4, true));
    assert(!indianos_user_fault_eligible(14, 0x2b, 4, false));
    assert(!indianos_user_fault_eligible(14, 0x33, 4, true));
    assert(!indianos_user_fault_eligible(8, 0x2b, 0, true));
    assert(!indianos_user_fault_eligible(12, 0x2b, 0, true));
    puts("IndianOS ring-3 exception isolation policy host tests passed");
    return 0;
}
