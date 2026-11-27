Feature: The job's colour mode
  Transparency declares its blend space, in the output intent's family; spot inks are on the allowed list; a CMYK job
  has no RGB images (RGB vector artwork is advisory), an RGB job no CMYK artwork, a gray job only gray (K-only CMYK
  when allowed); embedded ICC profiles are sound and agree with the job.

  Scenario: Transparency with a matching blend space
    Given the fixture "transparent-matching"
    When it is verified for print
    Then it passes

  Scenario: Transparency blending in the wrong family
    Given the fixture "transparent-mismatch"
    When it is verified for print
    Then check "blend-space" fails on page 1

  Scenario: A spot ink not on the list
    Given the fixture "spot-ink"
    When it is verified for print with '{"allow_spots": []}'
    Then check "spot-ink" fails on page 1

  Scenario: An RGB image in a CMYK job
    Given the fixture "rgb-image"
    When it is verified for print
    Then check "colour-space" fails on page 1

  Scenario: CMYK artwork in an RGB job
    Given the fixture "cmyk-vector"
    When it is verified for print with '{"colour_mode": "rgb"}'
    Then check "rgb-only" fails on page 1

  Scenario: Two RGB profiles in an RGB job
    Given the fixture "mixed-rgb-profiles"
    When it is verified for print with '{"colour_mode": "rgb"}'
    Then check "icc-profile" fails

  Scenario: Gray artwork in a gray job
    Given the fixture "gray-vector"
    When it is verified for print with '{"colour_mode": "gray"}'
    Then it passes

  Scenario: K-only CMYK artwork in a gray job that allows it
    Given the fixture "gray-k-only-vector"
    When it is verified for print with '{"colour_mode": "gray", "allow_k_only": true}'
    Then it passes

  Scenario: An RGB output intent in a gray job
    Given the fixture "gray-rgb-intent"
    When it is verified for print with '{"colour_mode": "gray"}'
    Then check "gray-only" fails

  Scenario: A CMYK image with C, M or Y ink in a gray job
    Given the fixture "gray-cmyk-image-not-k-only"
    When it is verified for print with '{"colour_mode": "gray", "allow_k_only": true}'
    Then check "gray-only" fails on page 1
