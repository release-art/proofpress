Feature: PDF/X-4 6.8, filters
  Streams use only the filters of the PDF 1.6 Reference, by their full names, and neither LZW nor Crypt.

  Scenario Outline: A forbidden filter fails its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture               | check                   |
      | x4-lzw                | x4.filter.no-lzw        |
      | x4-filter-abbreviated | x4.filter.standard-only |

  Scenario: Content under the Crypt filter fails 6.8 and cannot be read
    Given the fixture "x4-crypt-filter"
    When it is verified as PDF/X-4
    Then check "x4.filter.no-crypt" fails
    And check "content.damaged" fails
