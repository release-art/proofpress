Feature: PDF/X-4 6.25, architectural limits
  The file stays within Table C.1 of the PDF 1.6 Reference.

  Scenario: One indirect object too many fails
    Given the fixture "x4-too-many-objects"
    When it is verified as PDF/X-4
    Then check "x4.limits.objects" fails
    And no check fails but "x4.limits.objects"

  Scenario Outline: One value or construct beyond Table C.1 fails its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture            | check              |
      | x4-limit-integer   | x4.limits.lexical  |
      | x4-limit-name      | x4.limits.lexical  |
      | x4-limit-nesting   | x4.limits.graphics |
      | x4-limit-devicen   | x4.limits.graphics |
