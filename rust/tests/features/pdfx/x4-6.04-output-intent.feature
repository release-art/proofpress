Feature: PDF/X-4 6.4.1 and 6.4.2.1, the output intent
  Exactly one output intent is GTS_PDFX; it names its condition in a text string and embeds a printer profile, version
  2 or 4, for gray, RGB or CMYK, and no external one.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-no-output-intent | x4.output-intent.present |
      | x4-intent-not-pdfx | x4.output-intent.present |
      | x4-two-pdfx-intents | x4.output-intent.unique |
      | x4-intent-no-identifier | x4.output-intent.identifier-present |
      | x4-intent-identifier-not-text | x4.output-intent.identifier-text-string |
      | x4-intent-registry-not-url | x4.output-intent.registry-name-url |
      | x4-intent-no-profile | x4.output-intent.dest-profile-present |
      | x4-intent-profile-ref | x4.output-intent.no-dest-profile-ref |
      | x4-intent-no-condition | x4.output-intent.output-condition |
      | x4-intent-profile-v5 | x4.output-intent.profile-version |
      | x4-intent-profile-monitor | x4.output-intent.profile-class-prtr |
      | x4-intent-profile-lab | x4.output-intent.channels |
