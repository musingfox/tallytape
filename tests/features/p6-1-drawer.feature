# language: en

# ----------------------------------------------------------------------------
# Tag conventions
# ----------------------------------------------------------------------------
#   @dual      — Runs against BOTH layers:
#                 - API:  cucumber-rust → tallytape-core repos & events
#                 - UI:   playwright-bdd → React drawer with bun:sqlite-backed
#                         mockIPC against an isolated HOME-overridden DB.
#                Step text is intent-level so the same scenario binds to two
#                different step implementations.
#   @ui-only   — Presentation / animation behaviours with no API surface
#                (collapse / expand, highlight fade).
#   @api-only  — Reserved for data-layer invariants with no surface impact.
#
# Glossary (read each at the right layer):
#   "I see N receipts"               API: list_receipt_summaries returns N
#                                    UI:  N receipt rows in the DOM
#   "highlighted as new"             API: a `receipt-added` event was emitted
#                                         for that receipt
#                                    UI:  the row has `data-pending="true"`
#   "the receipt list is visible"    UI:  <table> is in the DOM
#   "the receipt list is hidden"     UI:  drawer aria-expanded="false" and
#                                         <table> is not rendered
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
  Scenario: Opening the app before recording any sessions
    Given I have not run any Claude Code sessions yet
    When I open the dashboard
    Then I see no receipts


  @ui-only
  Scenario: An empty surface invites the user to start
    Given I have not run any Claude Code sessions yet
    When I open the dashboard
    Then an empty-state message invites me to start


  @dual
  Scenario: Noticing a new session finish while the app is open
    Given the dashboard is showing my 2 most recent receipts
    When a new Claude Code session finishes
    Then the new receipt appears highlighted as new
    And I now see 3 receipts in total


  @ui-only
  Scenario: The new-arrival highlight fades after the entry animation
    Given the dashboard is showing my 2 most recent receipts
    When a new Claude Code session finishes
    And I wait for the entry animation to settle
    Then no receipt is highlighted as new


  @ui-only
  Scenario: Focusing on the summary by collapsing the receipt list
    Given the dashboard is showing 3 receipts
    When I collapse the receipt drawer
    Then the receipt list is hidden
    And the drawer header remains so I can re-open it
    When I re-open the receipt drawer
    Then the receipt list is visible again
