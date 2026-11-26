Feature: Spot colour definitions
  A spot ink has one look: the same names in the same alternate space give the same colour at full tint.

  Scenario: One spot ink, one definition
    Given the fixture "spot-colour"
    When it is verified for print
    Then it passes

  Scenario: One spot ink, two definitions
    Given the fixture "spot-conflict"
    When it is verified for print
    Then check "spot-definition" fails
