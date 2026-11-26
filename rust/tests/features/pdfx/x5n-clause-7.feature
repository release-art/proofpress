Feature: PDF/X-5n (ISO 15930-8 clause 7)
  The output intent refers to an n-colorant ICC v4 profile, beside the file or embedded in its URL file
  specification, whose colorants /ColorantTable names; device spaces stand for those colorants or are not used.

  Scenario Outline: An X-5n file with its n-colorant profile passes
    Given the fixture "<fixture>"
    When it is verified as PDF/X-5n
    Then it passes

    Examples:
      | fixture           |
      | x5n-good          |
      | x5n-good-6        |
      | x5n-good-embedded |

  Scenario: Verified alone, the rows that need the profile are not evaluated
    Given the fixture "x5n-good"
    When it is verified alone as PDF/X-5n
    Then check "x5.n-colorant-table-matches-profile" is not evaluated
    And no check fails

  Scenario Outline: One defect fails its row
    Given the fixture "<fixture>"
    When it is verified as PDF/X-5n
    Then check "<check>" fails

    Examples:
      | fixture                   | check                               |
      | x5n-profile-cs-cmyk       | x5.n-profilecs-xclr                 |
      | x5n-no-colorant-table     | x5.n-colorant-table-present         |
      | x5n-colorant-table-order  | x5.n-colorant-table-matches-profile |
      | x5n-url-mac               | x5.n-urls-ef-allowed                |
      | x5n-profile-v2            | x5.n-profile-iso15076               |
      | x5n-no-black              | x5.n-devicegray-needs-black         |
      | x5n-no-black              | x5.n-devicecmyk-needs-cmyk          |
      | x5n-rgb                   | x5.n-no-devicergb                   |
      | x5n-devicen-nchannel      | x5.n-devicen-subtype                |
      | x5n-devicen-process       | x5.n-devicen-no-process             |
      | x5n-transparency-no-group | x5.n-transparency-group-cs          |
      | x5n-trapnet-pcm-cmyk      | x5.n-trapnet-pcm                    |
      | x5n-trapnet-separations   | x5.n-trapnet-separations            |
