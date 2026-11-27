Feature: PDF/X-4 6.19 and 6.23, content streams
  Content uses only the operators of the PDF 1.6 Reference, preferably without BX/EX, names one of the four rendering
  intents, and can be read to its end, inline images included.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-bx-ex | x4.content.no-bx-ex |
      | x4-ri-unknown | x4.intent.known-values |
      | x4-gstate-ri-unknown | x4.intent.known-values |

  Scenario: An undefined operator is a PDF error, and a PDF/X-4 one even inside BX/EX
    Given the fixture "x4-unknown-operator"
    When it is verified as PDF/X-4
    Then check "content.operator" fails
    And check "x4.content.known-operators" fails

  Scenario: Inside BX/EX the PDF accepts it, PDF/X-4 does not
    Given the fixture "x4-operator-in-bx"
    When it is verified as PDF/X-4
    Then check "x4.content.known-operators" fails
    And check "x4.content.no-bx-ex" fails

  Scenario Outline: An error in reading the file raises its check and, at PDF/X-4, 6.1.1's
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And check "x4.pdf-reference.conforms" fails
    And no check fails but "<check>" and "x4.pdf-reference.conforms"

    Examples:
      | fixture | check |
      | x4-inline-image-unknown-end | content.damaged |
      | x4-content-unreadable | content.damaged |

  Scenario: Past one of the reader's limits the rest of the content is not read
    Given the fixture "x4-content-limit"
    When it is verified as PDF/X-4
    Then check "content.limit" fails
    And check "x4.limits.graphics" fails
