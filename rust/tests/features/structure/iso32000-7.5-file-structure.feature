Feature: ISO 32000-2 7.5 and 7.6, file structure
  The reader refuses a file only when nothing can be read: no header, no startxref, an unreadable cross-reference,
  no catalog, encryption, or a limit reached. Other irregularities are findings on a file that still opens.

  Scenario Outline: A file-level defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture          | check              |
      | x4-header-offset | file.header-offset |
      | x4-after-eof     | file.eof           |

  Scenario Outline: An error in reading the file raises its check and, at PDF/X-4, 6.1.1's
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And check "x4.pdf-reference.conforms" fails
    And no check fails but "<check>" and "x4.pdf-reference.conforms"

    Examples:
      | fixture          | check              |
      | x4-no-header     | file.header        |
      | x4-truncated     | file.startxref     |
      | x4-bad-xref      | file.xref          |
      | x4-no-root       | file.trailer       |
      | x4-many-updates  | file.limit         |
