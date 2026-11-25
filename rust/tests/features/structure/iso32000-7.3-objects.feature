Feature: ISO 32000-2 7.3 and 7.4, objects, streams and filters
  Byte-level defects are findings on a value that is still read; damage leaves the object as a gap.

  Scenario Outline: An object-level defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture                    | check                     |
      | x4-odd-hex                 | syntax.hex-digits         |
      | x4-octal-overflow          | syntax.octal-escape       |
      | x4-bad-name-escape         | syntax.name-escape        |
      | x4-duplicate-key           | syntax.duplicate-key      |
      | x4-no-eol-before-endstream | stream.eol                |
      | x4-damaged-object          | object.damaged            |
      | x4-dangling-reference      | object.dangling-reference |

  Scenario: An unknown filter is a finding wherever it appears
    Given the fixture "x4-unknown-filter"
    When it is verified as PDF/X-4
    Then check "stream.filter" fails
    And check "x4.filter.standard-only" fails
