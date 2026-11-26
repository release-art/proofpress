Feature: PDF/X-4p (ISO 15930-7 Annex A)
  The PDF/X intent refers to an external profile; a file set is the file with that profile, found at its /URLs
  beside the PDF and confirmed by /CheckSum. Nothing is fetched over a network.

  Scenario: A PDF/X-4p file with its external profile passes
    Given the fixture "x4p-good"
    When it is verified as PDF/X-4p
    Then it passes

  Scenario: Verified alone, the rows that need the external profile are not evaluated
    Given the fixture "x4p-good"
    When it is verified alone as PDF/X-4p
    Then check "x4p.fileset.checksum-matches" is not evaluated
    And check "x4p.fileset.fields-match" is not evaluated
    And check "x4p.fileset.profile-valid" is not evaluated
    And no check fails

  Scenario Outline: One defect in the reference fails its row
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4p
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture              | check                             |
      | x4p-embedded-profile | x4p.output-intent.no-dest-profile |
      | x4p-no-ref           | x4p.output-intent.dest-profile-ref |
      | x4p-no-profile-name  | x4p.ref.profile-name              |
      | x4p-url-string       | x4p.ref.urls                      |
      | x4p-url-embedded     | x4p.ref.urls-no-ef-platform       |
      | x4p-colorant-table   | x4p.ref.no-colorant-table         |
      | x4p-wrong-name       | x4p.fileset.fields-match          |

  Scenario Outline: Without the profile the checksum names, the file set fails, and what needs it is not evaluated
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4p
    Then check "x4p.fileset.checksum-matches" fails
    And check "x4p.fileset.profile-valid" is not evaluated
    And no check fails but "x4p.fileset.checksum-matches"

    Examples:
      | fixture             |
      | x4p-wrong-checksum  |
      | x4p-missing-profile |

  Scenario: A profile only a network URL could hold is not evaluated
    Given the fixture "x4p-network-only"
    When it is verified as PDF/X-4p
    Then check "x4p.fileset.checksum-matches" is not evaluated
    And no check fails

  Scenario: The external profile is held to 6.4.2.1
    Given the fixture "x4p-not-a-profile"
    When it is verified as PDF/X-4p
    Then check "x4p.fileset.profile-valid" fails
    And no check fails but "x4p.fileset.profile-valid" and "x4p.fileset.fields-match"

  Scenario Outline: A CMYK ICCBased space that is the external profile fails 6.4.3.3, alone or in the file set
    Given the fixture "x4p-cmyk-is-intent"
    When <how>
    Then check "x4.icc.cmyk-not-intent-profile" fails

    Examples:
      | how                               |
      | it is verified as PDF/X-4p        |
      | it is verified alone as PDF/X-4p  |
