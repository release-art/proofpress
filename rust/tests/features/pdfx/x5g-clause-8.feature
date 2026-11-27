Feature: PDF/X-5g and X-5pg (ISO 15930-8 clauses 8 and 9)
  A container places pages of other files through form proxies with /Ref. As a file set, each target is read beside
  the container, compared with its reference and verified at the level it claims.

  Scenario Outline: A container with a conforming target passes
    Given the fixture "<fixture>"
    When it is verified as PDF/X-<level>
    Then it passes

    Examples:
      | fixture   | level |
      | x5g-good  | 5g    |
      | x5pg-good | 5pg   |

  Scenario: Verified alone, the rows that need the targets are not evaluated
    Given the fixture "x5g-good"
    When it is verified alone as PDF/X-5g
    Then check "x5.g-target-conforms" is not evaluated
    And check "x5.g-target-identity" is not evaluated
    And no check fails

  Scenario Outline: One defect in the container or its target fails its row
    Given the fixture "<fixture>"
    When it is verified as PDF/X-5g
    Then check "<check>" fails

    Examples:
      | fixture                    | check                          |
      | x5g-wrong-version          | x5.identification-value        |
      | x5g-ref-no-id              | x5.g-ref-id-required           |
      | x5g-proxy-no-metadata      | x5.g-proxy-metadata            |
      | x5g-rendition-wrong-case   | x5.g-proxy-rendition-of        |
      | x5g-filespec-not-embedded  | x5.g-ref-filespec-external     |
      | x5g-target-missing         | x5.g-target-identity           |
      | x5g-target-wrong-id        | x5.g-target-identity           |
      | x5g-target-page-missing    | x5.g-target-page-exists        |
      | x5g-target-nonconforming   | x5.g-target-conforms           |
      | x5g-target-ocg             | x5.g-target-no-ocg             |
      | x5g-target-other-condition | x5.g-same-printing-condition   |
      | x5g-target-x2              | x5.g-target-conformance-level  |
      | x5g-cycle                  | x5.g-no-circular-refs          |

  Scenario Outline: What cannot be confirmed without a network or a choice is not evaluated
    Given the fixture "<fixture>"
    When it is verified as PDF/X-5g
    Then check "<check>" is not evaluated

    Examples:
      | fixture                  | check                      |
      | x5g-target-network       | x5.g-target-identity       |
      | x5g-target-other-version | x5.g-target-version-policy |

  Scenario: An X-1a target is verified at the level it claims (L-Q10)
    Given the fixture "x5g-target-x1a"
    When it is verified as PDF/X-5g
    Then no check fails but "x5.g-target-xmp-ids"
