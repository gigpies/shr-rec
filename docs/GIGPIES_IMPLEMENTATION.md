# SHR REC: plan and implementation

SHR REC owns bounded raw capture writing, progress, finalization and recovery.
GigPies supplies source-clock samples and host lifecycle; Desk presents operator state.

## Delivered contract and evidence

REC-01 lifecycle correlation and REC-02 retained observer/progress are implemented
with actual GigPies use. [Raw recorder contract](RAW_RECORDER.md),
[header](../include/shr_rec.h) and [dated evidence](STATUS.md) describe exact PCM24,
all-stem progress, completion, faults and recovery. Written frames do not prove
power-loss durability; unavailable durability remains unknown.

## Next work

Complete integrated operator operation is shared GP-REC; keep its plan/progress in
GigPies after checking which host actions already exist. A new provider task here
requires a concrete missing writer/observer behavior, not the standalone shell gap.
Storage/power-loss and sustained physical capture contribute to GP-H1/H3.
Standalone recorder UI, take format and synchronized player belong to [PLAN](PLAN.md)
and are not GigPies runtime integration requirements.

## Tracking and verification

This is the owning plan for module-only GigPies work. Keep each new task's plan,
implementation state, checklist, evidence and next action together here. Shared
integration tasks live only in the [GigPies integration plan](https://github.com/gigpies/gigpies/blob/main/docs/development/MODULE_IMPLEMENTATION_PLAN.md);
link to their cards instead of copying status. Follow its task lifecycle and this
repository's AGENTS.md. STATUS/acceptance files hold dated evidence, not another queue.
Preserve closed milestone details in the linked archive; do not reopen old launch cards.
A documentation reconciliation does not rerun tests or qualify hardware.

[Historical plan and milestone evidence](archive/tracking-before-2026-10-09/GIGPIES_IMPLEMENTATION.md). Its launch instructions are retired.
