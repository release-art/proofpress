Feature: PDF/X-4 6.9, trapping
  The trapped state is in XMP (X4-Q1), and is True or False.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-xmp-no-trapped | x4.trapped.present |
      | x4-trapped-unknown | x4.trapped.true-or-false |
