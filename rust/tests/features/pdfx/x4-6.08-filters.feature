Feature: PDF/X-4 6.8, filters
  Streams use only the filters of the PDF 1.6 Reference, by their full names, and neither LZW nor Crypt; JPEG and JBIG2
  data is valid at the marker and segment level.

  Scenario Outline: A forbidden filter fails its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture               | check                   |
      | x4-lzw                | x4.filter.no-lzw        |
      | x4-filter-abbreviated | x4.filter.standard-only |
      | x4-dct-truncated      | x4.image.dct-valid      |
      | x4-jbig2-truncated    | x4.image.jbig2-valid    |

  Scenario: Content under the Crypt filter fails 6.8 and cannot be read
    Given the fixture "x4-crypt-filter"
    When it is verified as PDF/X-4
    Then check "x4.filter.no-crypt" fails
    And check "content.damaged" fails
