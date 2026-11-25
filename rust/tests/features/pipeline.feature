Feature: The pipeline around a tier
  Every tier publishes its events to listeners, called one after another. First-order observers report what they see
  before any second-order reader builds on it, so a report of an event always comes first.

  Scenario: Observers hear an event before readers
    Given an observer "syntax check" and a reader "next tier"
    When an event is published
    Then "syntax check" hears it before "next tier"

  Scenario: Every listener hears every event once
    Given an observer "syntax check" and a reader "next tier"
    When 3 events are published
    Then "syntax check" heard 3 events
    And "next tier" heard 3 events
