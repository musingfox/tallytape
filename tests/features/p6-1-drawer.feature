# language: en

# ----------------------------------------------------------------------------
# E2E scope policy
# ----------------------------------------------------------------------------
# This feature file covers ONLY the happy path and the most critical end-to-end
# flow. Presentation details (empty-state copy, collapse/expand interaction,
# animation fade) live in component tests under
# `src/components/__tests__/ReceiptDrawer.test.tsx` — they don't belong here.
#
# Tag conventions
#   @dual — Runs against BOTH layers:
#             - API:  cucumber-rust → tallytape-core repos & events
#             - UI:   playwright-bdd → React drawer with bun:sqlite-backed
#                     mockIPC against an isolated HOME-overridden DB.
#           Step text is intent-level so the same scenario binds to two
#           different step implementations.
#
# Glossary (read each at the right layer):
#   "I see N receipts"               API: list_receipts returns N
#                                    UI:  N receipt rows in the DOM
#   "highlighted as new"             API: a `receipt-added` event was emitted
#                                         for that receipt
#                                    UI:  the row has `data-pending="true"`
# ----------------------------------------------------------------------------

Feature: Browsing recent Claude Code session receipts

  As a developer who runs Claude Code sessions across many projects,
  I want to glance at my recent receipts on the dashboard so I can audit
  my spend without leaving the editor flow.


  Background:
    Given a fresh tallytape data directory


  @dual
  Scenario: Catching up on recent sessions
    Given my last few days of work produced these sessions:
      | cwd                      | date       |
      | /Users/dev/project-alpha | 2026-05-15 |
      | /Users/dev/project-beta  | 2026-05-16 |
      | /Users/dev/project-gamma | 2026-05-17 |
    When I open the dashboard
    Then I see 3 receipts
    And the most recent receipt appears first


  @dual
  Scenario: Noticing a new session finish while the app is open
    Given the dashboard is showing my 2 most recent receipts
    When a new Claude Code session finishes
    Then the new receipt appears highlighted as new
    And I now see 3 receipts in total
